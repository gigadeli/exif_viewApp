//! メタデータの書き込み (上書き保存 / 別名で保存) と、書き込み可否の情報。

use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::commands::{with_exiftool, ExifToolState};
use crate::exiftool::{escape_value, ExifTool};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagChange {
    /// -G1 のグループ名 ("IFD0", "XMP-dc" など)。空なら ExifTool におまかせ
    pub group: String,
    pub name: String,
    /// None ならタグを削除
    pub value: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteOptions {
    /// true なら元ファイルを "<名前>_original" として残す (既にあれば上書きしない)
    pub backup: bool,
    /// true ならファイルの更新日時を変えない (-P)
    pub preserve_file_time: bool,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WriteResult {
    /// 書き込みが成功したか (変更なしで終わった場合も true)
    pub ok: bool,
    /// ファイルが実際に書き換えられたか
    pub updated: bool,
    /// 書き込み後のファイル (別名保存ならその保存先)
    pub path: String,
    pub errors: Vec<String>,
    /// 一部のタグが書き込めなかった場合など
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WritableInfo {
    /// 書き込み可能なタグ名 (グループは問わない)
    pub tags: Vec<String>,
    /// 書き込み可能な拡張子 (小文字)
    pub extensions: Vec<String>,
    /// ExifTool の代わりに自作ライターで書ける形式とタグ (AVI, MKV, WebM)
    pub native: crate::native::NativeWritable,
}

/// "-listw" などの出力 ("見出し:\n  A B C\n  D E") から単語を取り出す
fn parse_list(out: &str) -> Vec<String> {
    out.lines().skip(1).flat_map(|l| l.split_whitespace()).map(String::from).collect()
}

#[tauri::command]
pub async fn writable_info(
    app: AppHandle,
    state: State<'_, ExifToolState>,
) -> Result<WritableInfo, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        with_exiftool(&app, &state, |et| {
            let (tags, _) = et.execute(&["-listw"])?;
            let (exts, _) = et.execute(&["-listwf"])?;
            Ok(WritableInfo {
                tags: parse_list(&tags),
                extensions: parse_list(&exts).into_iter().map(|e| e.to_lowercase()).collect(),
                native: crate::native::writable(),
            })
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 「すべてのメタデータを削除」を表す特別な変更 (グループ空、名前 "all"、値 None)
fn is_remove_all(c: &TagChange) -> bool {
    c.group.is_empty() && c.name.eq_ignore_ascii_case("all") && c.value.is_none()
}

fn change_args(changes: &[TagChange]) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    // すべて削除は最初に行い、その後に他の変更を書き込む。
    // 画像の向きと色プロファイルは消すと見た目が変わるので、元ファイルからコピーし直して残す
    if changes.iter().any(is_remove_all) {
        args.extend(
            ["-all=", "-tagsFromFile", "@", "-IFD0:Orientation", "-ICC_Profile"].map(String::from),
        );
    }
    args.extend(changes.iter().filter(|c| !is_remove_all(c)).map(|c| {
            let key = if c.group.is_empty() { c.name.clone() } else { format!("{}:{}", c.group, c.name) };
            format!("-{key}={}", escape_value(c.value.as_deref().unwrap_or("")))
    }));
    // IPTC は文字コードを明示しないと日本語が化ける
    if changes.iter().any(|c| c.group.starts_with("IPTC")) {
        args.push("-IPTC:CodedCharacterSet=UTF8".into());
    }
    args
}

fn apply(
    et: &mut ExifTool,
    path: &str,
    changes: &[TagChange],
    backup: bool,
    preserve_file_time: bool,
) -> Result<WriteResult, String> {
    // AVI / MKV / WebM は ExifTool が書き込めないので自作ライターで書く
    if crate::native::detect(Path::new(path)).is_some() {
        return crate::native::apply(path, changes, backup, preserve_file_time);
    }
    // -sep: キーワードなどリスト型のタグは ", " 区切りで複数の値として書く (読み取り側の表示と同じ形式)
    let mut args: Vec<String> =
        vec!["-api".into(), "LargeFileSupport=1".into(), "-sep".into(), ", ".into()];
    // -E: 値の中の HTML 文字参照を解釈させる (改行を &#xa; で渡すため。escape_value と対)
    args.push("-E".into());
    if !backup {
        args.push("-overwrite_original".into());
    }
    if preserve_file_time {
        args.push("-P".into());
    }
    args.extend(change_args(changes));
    args.push(path.to_string());

    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    // 大きな動画はファイル全体を書き直すので、待つ上限を長くする
    let (out, err) = et.execute_timeout(&refs, crate::exiftool::WRITE_TIMEOUT)?;

    let mut result = WriteResult { path: path.to_string(), ..Default::default() };
    for line in err.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let msg = line.trim_end_matches(&format!(" - {path}")).to_string();
        if line.starts_with("Warning") {
            // すべて削除で向き・色プロファイルを残そうとしたが元から無かった場合の警告は、利用者には関係ない
            if changes.iter().any(is_remove_all) && line.starts_with("Warning: No writable tags set from") {
                continue;
            }
            result.warnings.push(msg);
        } else {
            result.errors.push(msg);
        }
    }
    result.updated = out.contains("1 image files updated");
    let unchanged = out.contains("1 image files unchanged");
    result.ok = result.errors.is_empty() && (result.updated || unchanged);
    if !result.ok && result.errors.is_empty() {
        result.errors.push(out.trim().to_string());
    }
    Ok(result)
}

#[tauri::command]
pub async fn write_metadata(
    app: AppHandle,
    state: State<'_, ExifToolState>,
    path: String,
    changes: Vec<TagChange>,
    options: WriteOptions,
) -> Result<WriteResult, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if !Path::new(&path).is_file() {
            return Err(format!("ファイルが見つかりません: {path}"));
        }
        with_exiftool(&app, &state, |et| {
            apply(et, &path, &changes, options.backup, options.preserve_file_time)
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 同じボリュームの作業用コピーを編集し、成功時だけ保存先を置き換える。
fn save_copy(
    source: &Path,
    dest: &Path,
    edit: impl FnOnce(&str) -> Result<WriteResult, String>,
) -> Result<WriteResult, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_ID: AtomicU64 = AtomicU64::new(0);
    let parent = dest.parent().ok_or("保存先フォルダがありません")?;
    let name = dest.file_name().ok_or("保存先の名前がありません")?;
    let dir = loop {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir = parent.join(format!(".exif-save-{}-{id}", std::process::id()));
        match std::fs::create_dir(&dir) {
            Ok(()) => break dir,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("作業フォルダを作れませんでした: {e}")),
        }
    };
    let staged = dir.join(name);
    let result = (|| {
        std::fs::copy(source, &staged).map_err(|e| format!("コピーできませんでした: {e}"))?;
        let mut result = edit(staged.to_str().ok_or("保存先のパスを読み取れません")?)?;
        if result.ok {
            std::fs::rename(&staged, dest).map_err(|e| format!("保存先を置き換えられませんでした: {e}"))?;
        }
        result.path = dest.to_string_lossy().into_owned();
        Ok(result)
    })();
    let _ = std::fs::remove_dir_all(dir);
    result
}

#[tauri::command]
pub async fn save_as(
    app: AppHandle,
    state: State<'_, ExifToolState>,
    path: String,
    dest: String,
    changes: Vec<TagChange>,
    options: WriteOptions,
) -> Result<WriteResult, String> {
    let state = state.inner().clone();
    let scope = app.asset_protocol_scope();
    tauri::async_runtime::spawn_blocking(move || {
        let same = match (Path::new(&path).canonicalize(), Path::new(&dest).canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        };
        if same {
            return Err("別名で保存するには、元ファイルと異なる保存先を選んでください".into());
        }
        let result = save_copy(Path::new(&path), Path::new(&dest), |staged| {
            if changes.is_empty() {
                return Ok(WriteResult { ok: true, ..Default::default() });
            }
            with_exiftool(&app, &state, |et| {
                apply(et, staged, &changes, false, options.preserve_file_time)
            })
        })?;
        if result.ok {
            let _ = scope.allow_file(&dest);
        }
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exiftool::ensure_extracted;

    #[test]
    fn save_copy_replaces_destination_only_after_success() {
        let dir = std::env::temp_dir().join(format!("exif-save-test-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let src = dir.join("source.jpg");
        let dest = dir.join("destination.jpg");
        std::fs::write(&src, b"source").unwrap();
        std::fs::write(&dest, b"original").unwrap();
        assert!(save_copy(&src, &dest, |_| Err("write failed".into())).is_err());
        assert_eq!(std::fs::read(&dest).unwrap(), b"original");
        assert!(!save_copy(&src, &dest, |_| Ok(WriteResult::default())).unwrap().ok);
        assert_eq!(std::fs::read(&dest).unwrap(), b"original");
        save_copy(&src, &dest, |path| {
            assert_eq!(std::fs::read(path).unwrap(), b"source");
            std::fs::write(path, b"edited").unwrap();
            Ok(WriteResult { ok: true, ..Default::default() })
        }).unwrap();
        assert_eq!(std::fs::read(&src).unwrap(), b"source");
        assert_eq!(std::fs::read(&dest).unwrap(), b"edited");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn change(group: &str, name: &str, value: Option<&str>) -> TagChange {
        TagChange { group: group.into(), name: name.into(), value: value.map(String::from) }
    }

    fn read(et: &mut ExifTool, path: &str, tag: &str) -> Option<String> {
        let (out, _) = et.execute(&["-j", "-G1", tag, path]).unwrap();
        let v: Vec<serde_json::Map<String, serde_json::Value>> = serde_json::from_str(&out).unwrap();
        v[0].iter().find(|(k, _)| k.as_str() != "SourceFile").map(|(_, v)| match v {
            serde_json::Value::String(s) => s.clone(),
            o => o.to_string(),
        })
    }

    /// EXIF_WRITE_DIR に 写真.jpg / 動画.mp4 / 動画.mkv のコピーがあるときだけ実行
    #[test]
    fn write_scenarios() {
        let Ok(dir) = std::env::var("EXIF_WRITE_DIR") else { return };
        let jpg = format!("{dir}/写真.jpg");
        let mp4 = format!("{dir}/動画.mp4");
        let mkv = format!("{dir}/動画.mkv");
        let base = std::env::temp_dir().join(format!("exif-app-test-ws-{}", std::process::id()));
        let mut et = ExifTool::start(&ensure_extracted(&base).unwrap()).unwrap();

        // 1. JPEG: 変更 + 追加 + 削除、バックアップあり
        let r = apply(
            &mut et,
            &jpg,
            &[
                change("IFD0", "Model", Some("EOS R5")),
                change("IFD0", "Artist", Some("山田 太郎")),
                change("XMP-dc", "Title", None),
                change("IPTC", "Keywords", Some("旅行")),
            ],
            true,
            false,
        )
        .unwrap();
        assert!(r.ok && r.updated, "{:?} {:?}", r.errors, r.warnings);
        assert_eq!(read(&mut et, &jpg, "-IFD0:Model").as_deref(), Some("EOS R5"));
        assert_eq!(read(&mut et, &jpg, "-IFD0:Artist").as_deref(), Some("山田 太郎"));
        assert_eq!(read(&mut et, &jpg, "-XMP-dc:Title"), None);
        assert_eq!(read(&mut et, &jpg, "-IPTC:Keywords").as_deref(), Some("旅行"));
        assert!(Path::new(&format!("{jpg}_original")).is_file(), "バックアップが無い");

        // 2. 書き込めないタグは警告、他は反映される
        let r = apply(
            &mut et,
            &jpg,
            &[change("Composite", "ImageSize", Some("1x1")), change("IFD0", "Make", Some("Sony"))],
            false,
            true,
        )
        .unwrap();
        assert!(r.ok && r.updated);
        assert!(!r.warnings.is_empty(), "警告が出ていない");
        println!("warnings: {:?}", r.warnings);
        assert_eq!(read(&mut et, &jpg, "-IFD0:Make").as_deref(), Some("Sony"));

        // 3. 何も変わらない書き込み
        let r = apply(&mut et, &jpg, &[change("IFD0", "Make", Some("Sony"))], false, false).unwrap();
        println!("same value: ok={} updated={} {:?}", r.ok, r.updated, r.warnings);
        assert!(r.ok);

        // 3b. キーワード (リスト型) は ", " 区切りで複数の値になる
        let r = apply(&mut et, &jpg, &[change("XMP-dc", "Subject", Some("旅行, 海, 夏"))], false, false).unwrap();
        assert!(r.ok);
        let (out, _) = et.execute(&["-j", "-XMP-dc:Subject", &jpg]).unwrap();
        assert!(out.contains(r#"["旅行","海","夏"]"#), "{out}");

        // 4. MP4 (QuickTime) のタイトル
        let r = apply(&mut et, &mp4, &[change("Keys", "Title", Some("動画のタイトル"))], false, false).unwrap();
        assert!(r.ok && r.updated, "{:?}", r.errors);
        assert_eq!(read(&mut et, &mp4, "-Keys:Title").as_deref(), Some("動画のタイトル"));

        // 4b. 画像の GPS (南緯・西経・高度)
        let r = apply(
            &mut et,
            &jpg,
            &[
                change("GPS", "GPSLatitude", Some("33.8688")),
                change("GPS", "GPSLatitudeRef", Some("S")),
                change("GPS", "GPSLongitude", Some("151.2093")),
                change("GPS", "GPSLongitudeRef", Some("W")),
                change("GPS", "GPSAltitude", Some("12.5")),
                change("GPS", "GPSAltitudeRef", Some("Above Sea Level")),
            ],
            false,
            false,
        )
        .unwrap();
        assert!(r.ok && r.warnings.is_empty(), "{:?} {:?}", r.errors, r.warnings);
        let (out, _) = et.execute(&["-j", "-n", "-Composite:GPSLatitude", "-Composite:GPSLongitude", "-Composite:GPSAltitude", &jpg]).unwrap();
        let v: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
        assert!((v[0]["GPSLatitude"].as_f64().unwrap() + 33.8688).abs() < 1e-6, "{out}");
        assert!((v[0]["GPSLongitude"].as_f64().unwrap() + 151.2093).abs() < 1e-6, "{out}");
        assert!((v[0]["GPSAltitude"].as_f64().unwrap() - 12.5).abs() < 1e-6, "{out}");

        // 4c. 動画の GPS
        let r = apply(
            &mut et,
            &mp4,
            &[
                change("Keys", "GPSCoordinates", Some("35.681, 139.767, 10")),
                change("UserData", "GPSCoordinates", Some("35.681, 139.767, 10")),
            ],
            false,
            false,
        )
        .unwrap();
        assert!(r.ok, "{:?}", r.errors);
        assert!(read(&mut et, &mp4, "-UserData:GPSCoordinates").unwrap().starts_with("35 deg 40' 51.60\" N"));

        // 4d. すべて削除 + 同時に作成者を設定。画像の向きは残る
        apply(&mut et, &jpg, &[change("IFD0", "Orientation", Some("Rotate 90 CW"))], false, false).unwrap();
        let r = apply(
            &mut et,
            &jpg,
            &[change("IFD0", "Artist", Some("削除後")), change("", "all", None)],
            false,
            false,
        )
        .unwrap();
        assert!(r.ok, "{:?} {:?}", r.errors, r.warnings);
        assert_eq!(read(&mut et, &jpg, "-IFD0:Orientation").as_deref(), Some("Rotate 90 CW"));
        assert_eq!(read(&mut et, &jpg, "-IFD0:Artist").as_deref(), Some("削除後"));
        assert_eq!(read(&mut et, &jpg, "-GPS:GPSLatitude"), None);
        assert_eq!(read(&mut et, &jpg, "-XMP-dc:Subject"), None);
        assert_eq!(read(&mut et, &jpg, "-IFD0:Make"), None);
        let r = apply(&mut et, &mp4, &[change("", "all", None)], false, false).unwrap();
        assert!(r.ok, "動画のすべて削除: {:?} {:?}", r.errors, r.warnings);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert_eq!(read(&mut et, &mp4, "-UserData:GPSCoordinates"), None);

        // 5. MKV は自作ライターで書ける
        let r = apply(&mut et, &mkv, &[change("Info", "Title", Some("x"))], false, false).unwrap();
        assert!(r.ok && r.updated, "{:?}", r.errors);
        assert_eq!(read(&mut et, &mkv, "-Info:Title").as_deref(), Some("x"));

        drop(et);
        let _ = std::fs::remove_dir_all(&base);
    }
}
