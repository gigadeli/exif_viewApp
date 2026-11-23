//! フロントエンドから呼ぶ Tauri コマンド。

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager, State};

use crate::exiftool::{self, ExifTool};

/// ExifTool プロセスは 1 つだけ。初回のコマンド実行時に展開・起動する。
#[derive(Default, Clone)]
/// 2 つ目は起動中の ExifTool のプロセス ID (終了時、処理中でロックを取れなくても止められるように)
pub struct ExifToolState(pub Arc<Mutex<Option<ExifTool>>>, pub Arc<std::sync::atomic::AtomicU32>);

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
        let et = ExifTool::start(&exe)?;
        state.1.store(et.pid(), std::sync::atomic::Ordering::SeqCst);
        *guard = Some(et);
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

    match read_json(et, path) {
        Ok((obj, err)) => {
            meta.warnings = err.lines().filter(|l| !l.trim().is_empty()).map(String::from).collect();
            match obj {
                Some(obj) => meta.tags = parse_tags(&obj),
                None => {
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

/// 1 ファイルのタグを ExifTool の JSON (グループ付きのキー) で読む。画面表示と書き出しで共通。
/// AVI の INFO の文字コードはここで補正する。返り値は (タグ, ExifTool のエラー出力)
fn read_json(et: &mut ExifTool, path: &str) -> Result<(Option<serde_json::Map<String, Value>>, String), String> {
    // -G1: 詳細なグループ名 / -a: 重複タグも出す / -s: タグ名で出力 / -j: JSON
    let mut args = vec!["-j", "-G1", "-a", "-s", "-api", "LargeFileSupport=1"];
    // AVI の INFO は既定では Windows-1252 として読まれる。値が UTF-8 なら UTF-8 として読む
    if crate::native::avi_info_is_utf8(Path::new(path)) {
        args.extend(["-api", "CharsetRIFF=UTF8"]);
    }
    args.push(path);
    let (out, err) = et.execute(&args)?;
    let mut obj = serde_json::from_str::<Vec<serde_json::Map<String, Value>>>(&out)
        .ok()
        .and_then(|list| list.into_iter().next());
    // 古い AVI の日本語などは ExifTool では読めないので、PC の文字コードで読み直した値に置き換える
    if let Some(obj) = obj.as_mut() {
        for (name, value) in crate::native::avi_legacy_values(Path::new(path)) {
            if let Some(v) = obj.get_mut(&format!("RIFF:{name}")) {
                *v = Value::String(value);
            }
        }
    }
    Ok((obj, err))
}

/// CSV の 1 マス。表計算ソフトで数式として実行されないよう、= + - @ などで始まる値 (数値を除く) は先頭に ' を付ける
fn csv_cell(v: &str) -> String {
    let formula = v.starts_with(['=', '+', '-', '@', '\t', '\r']) && v.trim().parse::<f64>().is_err();
    let v = if formula { format!("'{v}") } else { v.to_string() };
    if v.contains([',', '"', '\n', '\r']) || v.starts_with(' ') || v.ends_with(' ') {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v
    }
}

/// 読み取った結果を CSV にする (1 行 = 1 ファイル、列 = タグ。ExifTool の -csv と同じ並び)
fn to_csv(rows: &[serde_json::Map<String, Value>]) -> String {
    let mut columns: Vec<&str> = vec!["SourceFile"];
    for row in rows {
        for k in row.keys() {
            if !columns.contains(&k.as_str()) {
                columns.push(k);
            }
        }
    }
    let mut out = columns.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(",");
    out.push_str("\r\n");
    for row in rows {
        let line: Vec<String> = columns.iter().map(|c| row.get(*c).map(|v| csv_cell(&value_to_string(v))).unwrap_or_default()).collect();
        out.push_str(&line.join(","));
        out.push_str("\r\n");
    }
    out
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
        if format != "json" && format != "csv" {
            return Err(format!("未対応の形式: {format}"));
        }
        // 画面と同じ読み方 (AVI の文字コードの補正を含む) で 1 ファイルずつ読む
        let rows = with_exiftool(&app, &state, |et| {
            let mut rows = Vec::new();
            for p in &paths {
                let (obj, err) = read_json(et, p)?;
                rows.push(obj.ok_or_else(|| if err.trim().is_empty() { format!("読み取れませんでした: {p}") } else { err })?);
            }
            Ok(rows)
        })?;
        let bytes = if format == "csv" {
            [b"\xEF\xBB\xBF".as_slice(), to_csv(&rows).as_bytes()].concat()
        } else {
            serde_json::to_vec_pretty(&rows).map_err(|e| e.to_string())?
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

/// 終了時に ExifTool を止める。処理中 (ロックを取れない) なら待たずにプロセスを強制終了する
pub fn shutdown(state: &ExifToolState) {
    match state.0.try_lock() {
        Ok(mut guard) => {
            guard.take();
        }
        Err(_) => {
            let pid = state.1.load(std::sync::atomic::Ordering::SeqCst);
            #[cfg(windows)]
            if pid != 0 {
                crate::exiftool::kill_pid(pid);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_cells_are_safe() {
        assert_eq!(csv_cell("plain"), "plain");
        assert_eq!(csv_cell("a,b"), "\"a,b\"");
        assert_eq!(csv_cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_cell("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(csv_cell("@SUM(A1)"), "'@SUM(A1)");
        assert_eq!(csv_cell("-12.5"), "-12.5");
        assert_eq!(csv_cell("+cmd"), "'+cmd");
        assert_eq!(csv_cell("1 行目\n2 行目"), "\"1 行目\n2 行目\"");
    }

    #[test]
    fn csv_has_one_row_per_file() {
        let row = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), Value::String(v.to_string()))).collect();
        let csv = to_csv(&[row(&[("SourceFile", "a.jpg"), ("IFD0:Make", "X")]), row(&[("SourceFile", "b.avi"), ("RIFF:Title", "題")])]);
        assert_eq!(csv, "SourceFile,IFD0:Make,RIFF:Title\r\na.jpg,X,\r\nb.avi,,題\r\n");
    }
}
