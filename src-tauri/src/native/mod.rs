//! ExifTool が書き込めない形式 (AVI, MKV, WebM) のタグを自前で書き換える。
//! 設計は docs/native-writer.md を参照。読み取りは今までどおり ExifTool で行う。

mod ebml;
mod matroska;
mod riff;
#[cfg(test)]
mod file_tests;

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::write::{TagChange, WriteResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Avi,
    Matroska,
}

/// ファイルの先頭を見て、自作ライターで扱う形式かを判定する
pub fn detect(path: &Path) -> Option<Kind> {
    let mut head = [0u8; 64];
    let n = File::open(path).ok()?.read(&mut head).ok()?;
    let head = &head[..n];
    if n >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"AVI " {
        return Some(Kind::Avi);
    }
    if n >= 4 && head[0..4] == [0x1A, 0x45, 0xDF, 0xA3] {
        return matroska::is_matroska(path).then_some(Kind::Matroska);
    }
    None
}

pub fn avi_info_is_utf8(path: &Path) -> bool {
    detect(path) == Some(Kind::Avi) && riff::info_is_utf8(path)
}

/// フロントエンドに渡す「自作ライターで書けるもの」
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeWritable {
    /// ExifTool の File:FileType の値
    pub file_types: Vec<String>,
    /// グループ名 → 書き込めるタグ名
    pub tags: std::collections::BTreeMap<String, Vec<String>>,
}

pub fn writable() -> NativeWritable {
    let mut tags = std::collections::BTreeMap::new();
    tags.insert("RIFF".to_string(), riff::INFO_TAGS.iter().map(|(n, _)| n.to_string()).collect());
    tags.insert("Info".to_string(), matroska::INFO_NAMES.iter().map(|n| n.to_string()).collect());
    tags.insert(
        "Matroska".to_string(),
        matroska::SIMPLE_TAGS.iter().map(|(n, _)| n.to_string()).collect(),
    );
    NativeWritable { file_types: ["AVI", "MKV", "MKA", "WEBM"].map(String::from).to_vec(), tags }
}

/// 「すべてのメタデータを削除」(write.rs の is_remove_all と同じ判定)
fn is_remove_all(c: &TagChange) -> bool {
    c.group.is_empty() && c.name.eq_ignore_ascii_case("all") && c.value.is_none()
}

/// 日時の "YYYY:MM:DD HH:MM:SS" を "YYYY-MM-DD HH:MM:SS" にする (RIFF / Matroska の文字列の日時)
pub(crate) fn dash_date(v: &str) -> String {
    let b = v.as_bytes();
    if b.len() >= 10 && b[4] == b':' && b[7] == b':' && b[..4].iter().all(u8::is_ascii_digit) {
        format!("{}-{}-{}", &v[..4], &v[5..7], &v[8..])
    } else {
        v.to_string()
    }
}

/// 自作ライターで書き込む。backup / preserve_file_time は ExifTool の場合と同じ意味
pub fn apply(path: &str, changes: &[TagChange], backup: bool, preserve_file_time: bool) -> Result<WriteResult, String> {
    let src = Path::new(path);
    let kind = detect(src).ok_or("この形式には書き込めません")?;
    let mut result = WriteResult { path: path.to_string(), ..Default::default() };
    let remove_all = changes.iter().any(is_remove_all);
    let staged = staged_path(src)?;

    let outcome = match kind {
        Kind::Avi => {
            let (edits, unsupported) = riff::plan(changes);
            result.warnings.extend(unsupported);
            riff::write(src, &staged, &edits, remove_all)
        }
        Kind::Matroska => {
            let (edits, unsupported) = matroska::plan(changes);
            result.warnings.extend(unsupported);
            matroska::write(src, &staged, &edits, remove_all)
        }
    };
    match outcome {
        Ok(true) => {
            if let Err(e) = commit(src, &staged, backup, preserve_file_time) {
                let _ = std::fs::remove_file(&staged);
                return Err(e);
            }
            result.updated = true;
            result.ok = true;
        }
        Ok(false) => {
            let _ = std::fs::remove_file(&staged);
            result.ok = true;
        }
        Err(e) => {
            let _ = std::fs::remove_file(&staged);
            result.errors.push(e);
        }
    }
    Ok(result)
}

/// 元ファイルと同じフォルダの作業用ファイル名
fn staged_path(src: &Path) -> Result<PathBuf, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = src.parent().ok_or("フォルダがありません")?;
    let name = src.file_name().ok_or("ファイル名がありません")?.to_string_lossy();
    loop {
        let p = dir.join(format!(".{name}.exif-{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        if !p.exists() {
            return Ok(p);
        }
    }
}

/// 作業用ファイルで元ファイルを置き換える
fn commit(src: &Path, staged: &Path, backup: bool, preserve_file_time: bool) -> Result<(), String> {
    let meta = std::fs::metadata(src).map_err(|e| e.to_string())?;
    // 作成日時は元ファイルのものを引き継ぐ。更新日時は設定があれば元のまま
    {
        let f = std::fs::OpenOptions::new().write(true).open(staged).map_err(|e| e.to_string())?;
        let mut times = std::fs::FileTimes::new();
        if preserve_file_time {
            if let Ok(m) = meta.modified() {
                times = times.set_modified(m);
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileTimesExt;
            if let Ok(c) = meta.created() {
                times = times.set_created(c);
            }
        }
        let _ = f.set_times(times);
    }
    // バックアップは元ファイルの名前を変えて残す。ExifTool と同じく、既にあるバックアップは上書きしない
    let mut b = src.as_os_str().to_owned();
    b.push("_original");
    let b = PathBuf::from(b);
    let moved = backup && !b.exists();
    if moved {
        std::fs::rename(src, &b).map_err(|e| format!("バックアップを作れませんでした: {e}"))?;
    }
    std::fs::rename(staged, src).map_err(|e| {
        if moved {
            let _ = std::fs::rename(&b, src);
        }
        format!("元のファイルを置き換えられませんでした: {e}")
    })
}

/// 2 つのファイル範囲を作業用ファイルへコピーする小さな道具
pub(crate) fn copy_range(src: &mut File, dst: &mut impl std::io::Write, start: u64, len: u64) -> Result<(), String> {
    use std::io::{Seek, SeekFrom};
    src.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
    let copied = std::io::copy(&mut src.take(len), dst).map_err(|e| e.to_string())?;
    if copied != len {
        return Err("ファイルが途中で終わっています".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dash_dates() {
        assert_eq!(dash_date("2024:05:01 10:20:30"), "2024-05-01 10:20:30");
        assert_eq!(dash_date("2024-05-01"), "2024-05-01");
        assert_eq!(dash_date("abc"), "abc");
    }
}
