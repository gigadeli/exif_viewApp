//! バイナリのタグ (埋め込みサムネイル、メーカーノート、ICC プロファイルなど) の中身を見る・保存する。

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, State};

use crate::commands::{with_exiftool, ExifToolState};
use crate::exiftool::ExifTool;

/// 画像として表示するときに送る上限。これより大きい画像は先頭だけ送り、16 進表示にする
const IMAGE_LIMIT: usize = 50 * 1024 * 1024;
/// 16 進・テキスト表示で送る上限
const PREVIEW_LIMIT: usize = 64 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryData {
    /// 全体のバイト数
    pub size: usize,
    /// "image" (画像として表示できる) / "text" (UTF-8 の文字列) / "other"
    pub kind: &'static str,
    /// 画像なら MIME タイプ
    pub mime: Option<&'static str>,
    /// 中身 (base64)。画像なら全体、それ以外は先頭 PREVIEW_LIMIT バイトまで
    pub base64: String,
    /// 先頭だけを送った場合 true
    pub truncated: bool,
}

/// ExifTool から 1 つのタグのバイナリを取り出す
fn extract(et: &mut ExifTool, path: &str, group: &str, name: &str) -> Result<Vec<u8>, String> {
    let tag = if group.is_empty() { format!("-{name}") } else { format!("-{group}:{name}") };
    // -j と -b を併用すると、バイナリは "base64:..." として JSON に入る
    let (out, err) = et.execute(&["-j", "-b", &tag, path])?;
    let list: Vec<serde_json::Map<String, Value>> =
        serde_json::from_str(&out).map_err(|_| format!("読み取れませんでした: {}", err.trim()))?;
    let value = list
        .first()
        .and_then(|m| m.iter().find(|(k, _)| k.as_str() != "SourceFile").map(|(_, v)| v.clone()))
        .ok_or_else(|| format!("{group}:{name} が見つかりません"))?;
    match value {
        Value::String(s) => match s.strip_prefix("base64:") {
            Some(b64) => STANDARD.decode(b64).map_err(|e| e.to_string()),
            None => Ok(s.into_bytes()),
        },
        other => Ok(other.to_string().into_bytes()),
    }
}

/// 先頭のバイト列から画像の種類を判定する
fn image_mime(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else if b.starts_with(b"BM") && b.len() > 26 {
        Some("image/bmp")
    } else {
        None
    }
}

fn looks_like_text(b: &[u8]) -> bool {
    let head = &b[..b.len().min(PREVIEW_LIMIT)];
    // 途中で切った場合に最後の文字が欠けても判定できるよう、末尾の不完全な文字は無視する
    let valid = match std::str::from_utf8(head) {
        Ok(s) => s,
        Err(e) if e.error_len().is_none() => std::str::from_utf8(&head[..e.valid_up_to()]).unwrap(),
        Err(_) => return false,
    };
    !valid.is_empty() && valid.chars().all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
}

#[tauri::command]
pub async fn read_binary(
    app: AppHandle,
    state: State<'_, ExifToolState>,
    path: String,
    group: String,
    name: String,
) -> Result<BinaryData, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = with_exiftool(&app, &state, |et| extract(et, &path, &group, &name))?;
        let size = bytes.len();
        if let Some(mime) = image_mime(&bytes).filter(|_| size <= IMAGE_LIMIT) {
            return Ok(BinaryData { size, kind: "image", mime: Some(mime), base64: STANDARD.encode(&bytes), truncated: false });
        }
        let head = &bytes[..size.min(PREVIEW_LIMIT)];
        Ok(BinaryData {
            size,
            kind: if looks_like_text(&bytes) { "text" } else { "other" },
            mime: None,
            base64: STANDARD.encode(head),
            truncated: size > PREVIEW_LIMIT,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// バイナリのタグをそのままファイルに保存する。保存したバイト数を返す
#[tauri::command]
pub async fn save_binary(
    app: AppHandle,
    state: State<'_, ExifToolState>,
    path: String,
    group: String,
    name: String,
    dest: String,
) -> Result<usize, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = with_exiftool(&app, &state, |et| extract(et, &path, &group, &name))?;
        std::fs::write(&dest, &bytes).map_err(|e| format!("保存できませんでした: {e}"))?;
        Ok(bytes.len())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exiftool::ensure_extracted;

    #[test]
    fn detects_types() {
        assert_eq!(image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(image_mime(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
        assert_eq!(image_mime(b"hello"), None);
        assert!(looks_like_text("日本語のテキスト\nline".as_bytes()));
        assert!(!looks_like_text(&[0, 1, 2, 3]));
    }

    /// EXIF_BINARY_SAMPLE に ThumbnailImage を持つ JPEG を指定したときだけ実行
    #[test]
    fn extracts_thumbnail() {
        let Ok(sample) = std::env::var("EXIF_BINARY_SAMPLE") else { return };
        let base = std::env::temp_dir().join(format!("exif-app-test-b-{}", std::process::id()));
        let mut et = ExifTool::start(&ensure_extracted(&base).unwrap()).unwrap();
        let bytes = extract(&mut et, &sample, "IFD1", "ThumbnailImage").unwrap();
        assert_eq!(image_mime(&bytes), Some("image/jpeg"), "先頭: {:?}", &bytes[..4.min(bytes.len())]);
        assert!(bytes.len() > 100);
        drop(et);
        let _ = std::fs::remove_dir_all(&base);
    }
}
