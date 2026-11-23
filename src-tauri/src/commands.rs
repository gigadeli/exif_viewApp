//! フロントエンドから呼ぶ Tauri コマンド。

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager, State};

use crate::exiftool::{self, ExifTool};

/// ExifTool プロセスは 1 つだけ。初回のコマンド実行時に展開・起動する。
#[derive(Default, Clone)]
pub struct ExifToolState(pub Arc<Mutex<Option<ExifTool>>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    /// "EXIF:IFD0" のような -G1 のグループ名 (ExifTool の出力では "IFD0" など)
    pub group: String,
    pub name: String,
    pub value: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMetadata {
    pub path: String,
    pub file_name: String,
    pub tags: Vec<Tag>,
    /// 読み取りに失敗した場合のメッセージ
    pub error: Option<String>,
    /// ExifTool の警告 (読めたが問題がある場合)
    pub warnings: Vec<String>,
}

pub(crate) fn with_exiftool<T>(
    app: &AppHandle,
    state: &ExifToolState,
    f: impl FnOnce(&mut ExifTool) -> Result<T, String>,
) -> Result<T, String> {
    let mut guard = state.0.lock().map_err(|_| "内部エラー (lock)".to_string())?;
    if let Some(et) = guard.as_mut() {
        if !et.is_alive() {
            *guard = None; // 落ちていたら起動し直す
        }
    }
    if guard.is_none() {
        let base = app
            .path()
            .app_local_data_dir()
            .map_err(|e| e.to_string())?
            .join("exiftool");
        std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
        let exe = exiftool::ensure_extracted(&base)?;
        *guard = Some(ExifTool::start(&exe)?);
    }
    f(guard.as_mut().unwrap())
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(value_to_string).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

fn parse_tags(obj: &serde_json::Map<String, Value>) -> Vec<Tag> {
    obj.iter()
        .filter(|(k, _)| k.as_str() != "SourceFile")
        .map(|(k, v)| {
            let (group, name) = match k.rsplit_once(':') {
                Some((g, n)) => (g.to_string(), n.to_string()),
                None => (String::new(), k.clone()),
            };
            Tag { group, name, value: value_to_string(v) }
        })
        .collect()
}

fn read_one(et: &mut ExifTool, path: &str) -> FileMetadata {
    let file_name = Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    let mut meta = FileMetadata {
        path: path.to_string(),
        file_name,
        tags: Vec::new(),
        error: None,
        warnings: Vec::new(),
    };

    // -G1: 詳細なグループ名 / -a: 重複タグも出す / -s: タグ名で出力 / -j: JSON
    let mut args = vec!["-j", "-G1", "-a", "-s", "-api", "LargeFileSupport=1"];
    // AVI の INFO は既定では Latin-1 として読まれる。値が UTF-8 なら UTF-8 として読む
    if crate::native::avi_info_is_utf8(Path::new(path)) {
        args.extend(["-api", "CharsetRIFF=UTF8"]);
    }
    args.push(path);
    let result = et.execute(&args);
    match result {
        Ok((out, err)) => {
            meta.warnings = err.lines().filter(|l| !l.trim().is_empty()).map(String::from).collect();
            match serde_json::from_str::<Vec<serde_json::Map<String, Value>>>(&out) {
                Ok(list) if !list.is_empty() => meta.tags = parse_tags(&list[0]),
                _ => {
                    meta.error = Some(if err.trim().is_empty() {
                        "メタデータを読み取れませんでした".into()
                    } else {
                        err.trim().to_string()
                    })
                }
            }
        }
        Err(e) => meta.error = Some(e),
    }
    meta
}

#[tauri::command]
pub async fn read_metadata(
    app: AppHandle,
    state: State<'_, ExifToolState>,
    paths: Vec<String>,
) -> Result<Vec<FileMetadata>, String> {
    // 開いたファイルだけを asset プロトコル (プレビュー表示) で読めるようにする
    let scope = app.asset_protocol_scope();
    for p in &paths {
        let _ = scope.allow_file(p);
    }

    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        with_exiftool(&app, &state, |et| {
            Ok(paths.iter().map(|p| read_one(et, p)).collect())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn exiftool_version(
    app: AppHandle,
    state: State<'_, ExifToolState>,
) -> Result<String, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        with_exiftool(&app, &state, |et| {
            let (out, _) = et.execute(&["-ver"])?;
            Ok(out.trim().to_string())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// WebView で直接表示できない形式 (RAW, HEIC など) 用に、ファイルに埋め込まれた
/// プレビュー画像を data URL で返す。無ければ None。
#[tauri::command]
pub async fn get_embedded_preview(
    app: AppHandle,
    state: State<'_, ExifToolState>,
    path: String,
) -> Result<Option<String>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        with_exiftool(&app, &state, |et| {
            // -j と -b を併用すると、バイナリ値が "base64:..." として JSON に入る
            let (out, _) = et.execute(&[
                "-j", "-b", "-PreviewImage", "-JpgFromRaw", "-OtherImage", "-ThumbnailImage", &path,
            ])?;
            let list: Vec<serde_json::Map<String, Value>> =
                serde_json::from_str(&out).unwrap_or_default();
            // いちばん大きい (= 高解像度の) 画像を使う
            let best = list
                .first()
                .into_iter()
                .flat_map(|m| m.values())
                .filter_map(|v| v.as_str()?.strip_prefix("base64:"))
                .max_by_key(|b64| b64.len());
            Ok(best.map(|b64| format!("data:image/jpeg;base64,{b64}")))
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// メタデータを JSON / CSV でファイルに書き出す。
/// CSV は 1 行 = 1 ファイル、列 = タグ (ExifTool の -csv 形式)。Excel で開けるよう BOM を付ける。
#[tauri::command]
pub async fn export_metadata(
    app: AppHandle,
    state: State<'_, ExifToolState>,
    paths: Vec<String>,
    format: String,
    dest: String,
) -> Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let fmt = match format.as_str() {
            "json" => "-j",
            "csv" => "-csv",
            _ => return Err(format!("未対応の形式: {format}")),
        };
        let out = with_exiftool(&app, &state, |et| {
            let mut args = vec![fmt, "-G1", "-a", "-s", "-api", "LargeFileSupport=1"];
            args.extend(paths.iter().map(String::as_str));
            let (out, err) = et.execute(&args)?;
            if out.trim().is_empty() {
                return Err(if err.trim().is_empty() { "出力が空です".into() } else { err });
            }
            Ok(out)
        })?;
        let bytes = if fmt == "-csv" {
            [b"\xEF\xBB\xBF".as_slice(), out.as_bytes()].concat()
        } else {
            out.into_bytes()
        };
        std::fs::write(&dest, bytes).map_err(|e| format!("書き出せませんでした: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 起動時のコマンドライン引数で渡されたファイル (「プログラムから開く」や exe へのドロップ)
#[tauri::command]
pub fn startup_paths() -> Vec<String> {
    std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-') && Path::new(a).is_file())
        .collect()
}

/// 終了時に ExifTool を止める
pub fn shutdown(state: &ExifToolState) {
    if let Ok(mut guard) = state.0.lock() {
        guard.take();
    }
}
