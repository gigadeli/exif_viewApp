//! AVI (RIFF) の INFO タグ。設計は docs/native-writer.md の 3 章。
//!
//! 映像データ (LIST movi) より前の「ヘッダー領域」の中で INFO と JUNK を並べ替える。
//! 入りきらない場合だけ、ヘッダー領域を広げてファイルを書き直し、ずれた絶対位置を直す。

use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::{copy_range, dash_date};
use crate::write::TagChange;

/// ExifTool のタグ名 (グループ RIFF) と INFO のチャンク ID
pub const INFO_TAGS: &[(&str, &[u8; 4])] = &[
    ("Title", b"INAM"),
    ("Artist", b"IART"),
    ("Comment", b"ICMT"),
    ("Copyright", b"ICOP"),
    ("DateCreated", b"ICRD"),
    ("Keywords", b"IKEY"),
    ("Genre", b"IGNR"),
    ("Subject", b"ISBJ"),
    ("Software", b"ISFT"),
    ("Product", b"IPRD"),
    ("Source", b"ISRC"),
    ("Engineer", b"IENG"),
    ("Technician", b"ITCH"),
    ("Language", b"ILNG"),
    ("Country", b"ICNT"),
    ("Rating", b"IRTD"),
    ("Starring", b"ISTR"),
    ("Medium", b"IMED"),
    ("ArchivalLocation", b"IARL"),
    ("Commissioned", b"ICMS"),
];

/// ヘッダー領域として読み込む上限 (通常は数 KB〜数百 KB)
const MAX_REGION: u64 = 256 * 1024 * 1024;
/// ヘッダー領域を広げるときの単位と、次回のために残す余白
const GROW_ALIGN: u64 = 2048;
const GROW_SLACK: u64 = 1024;

#[derive(Debug)]
pub struct Edit {
    id: [u8; 4],
    value: Option<Vec<u8>>,
}

/// タグの変更を INFO の変更にする。書けないタグは警告の文にして返す
pub fn plan(changes: &[TagChange]) -> (Vec<Edit>, Vec<String>) {
    let mut edits = Vec::new();
    let mut unsupported = Vec::new();
    for c in changes {
        if c.group.is_empty() && c.name.eq_ignore_ascii_case("all") && c.value.is_none() {
            continue;
        }
        let found = (c.group.is_empty() || c.group == "RIFF")
            .then(|| INFO_TAGS.iter().find(|(n, _)| n.eq_ignore_ascii_case(&c.name)))
            .flatten();
        let Some((name, id)) = found else {
            unsupported.push(format!("Warning: 「{}:{}」は AVI に書き込めないタグです", c.group, c.name));
            continue;
        };
        let value = c.value.as_deref().filter(|v| !v.is_empty()).map(|v| {
            let v = if *name == "DateCreated" { dash_date(v) } else { v.to_string() };
            v.into_bytes()
        });
        edits.push(Edit { id: **id, value });
    }
    (edits, unsupported)
}

#[derive(Debug, Clone)]
struct Chunk {
    id: [u8; 4],
    list: Option<[u8; 4]>,
    off: u64,
    size: u32,
}

impl Chunk {
    fn total(&self) -> u64 {
        8 + self.size as u64 + (self.size & 1) as u64
    }
    fn is_list(&self, t: &[u8; 4]) -> bool {
        &self.id == b"LIST" && self.list.as_ref() == Some(t)
    }
}

struct Layout {
    file_len: u64,
    riff_size: u32,
    movi: Chunk,
    idx1: Option<Chunk>,
}

fn u32le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
fn u64le(b: &[u8]) -> u64 {
    u64::from_le_bytes(b[..8].try_into().unwrap())
}

fn io(e: std::io::Error) -> String {
    e.to_string()
}

/// 最初の RIFF の中のチャンクを読み、movi と idx1 の位置を調べる
fn parse(f: &mut File) -> Result<Layout, String> {
    let file_len = f.metadata().map_err(io)?.len();
    let mut head = [0u8; 12];
    f.seek(SeekFrom::Start(0)).map_err(io)?;
    f.read_exact(&mut head).map_err(|_| "AVI のヘッダーを読み取れません")?;
    if &head[0..4] != b"RIFF" || &head[8..12] != b"AVI " {
        return Err("AVI ファイルではありません".into());
    }
    let riff_size = u32le(&head[4..8]);
    let riff_end = (8 + riff_size as u64).min(file_len);
    let (mut movi, mut idx1) = (None, None);
    let mut pos = 12u64;
    while pos + 8 <= riff_end {
        let mut h = [0u8; 12];
        f.seek(SeekFrom::Start(pos)).map_err(io)?;
        let n = f.read(&mut h).map_err(io)?;
        if n < 8 {
            break;
        }
        let id: [u8; 4] = h[0..4].try_into().unwrap();
        let size = u32le(&h[4..8]);
        let list = (&id == b"LIST" && n >= 12).then(|| h[8..12].try_into().unwrap());
        let c = Chunk { id, list, off: pos, size };
        if c.is_list(b"movi") && movi.is_none() {
            movi = Some(c.clone());
        } else if &c.id == b"idx1" && movi.is_some() {
            idx1 = Some(c.clone());
        }
        pos += c.total();
    }
    let movi = movi.ok_or("AVI の映像データ (movi) が見つかりません")?;
    Ok(Layout { file_len, riff_size, movi, idx1 })
}

/// ヘッダー領域のチャンクを順に返す (相対位置、チャンク全体の長さ)
fn region_chunks(region: &[u8]) -> Result<Vec<(usize, usize)>, String> {
    let mut out = Vec::new();
    let mut p = 0usize;
    while p + 8 <= region.len() {
        let size = u32le(&region[p + 4..p + 8]) as usize;
        let total = 8 + size + (size & 1);
        if p + total > region.len() {
            // 最後のチャンクの埋め草が足りないだけなら許す
            if p + 8 + size == region.len() {
                out.push((p, region.len() - p));
                break;
            }
            return Err("AVI のヘッダーが壊れています".into());
        }
        out.push((p, total));
        p += total;
    }
    if p < region.len() && p + 8 > region.len() && region[p..].iter().any(|&b| b != 0) {
        return Err("AVI のヘッダーが壊れています".into());
    }
    Ok(out)
}

/// LIST INFO の中身 (ID と値)。値の末尾の NUL は取り除く
fn parse_info(chunk: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    let mut items = Vec::new();
    let end = (8 + u32le(&chunk[4..8]) as usize).min(chunk.len());
    let mut p = 12;
    while p + 8 <= end {
        let id: [u8; 4] = chunk[p..p + 4].try_into().unwrap();
        let size = u32le(&chunk[p + 4..p + 8]) as usize;
        let data_end = (p + 8 + size).min(end);
        let mut v = chunk[p + 8..data_end].to_vec();
        while v.last() == Some(&0) {
            v.pop();
        }
        items.push((id, v));
        p += 8 + size + (size & 1);
    }
    items
}

/// INFO チャンクを組み立てる。extra は最後の値の後ろに足す NUL の数 (偶数)
fn build_info(items: &[([u8; 4], Vec<u8>)], extra: usize) -> Vec<u8> {
    let mut body = b"INFO".to_vec();
    for (i, (id, v)) in items.iter().enumerate() {
        let mut d = v.clone();
        d.push(0);
        if i + 1 == items.len() {
            d.resize(d.len() + extra, 0);
        }
        body.extend_from_slice(id);
        body.extend_from_slice(&(d.len() as u32).to_le_bytes());
        body.extend_from_slice(&d);
        if d.len() % 2 == 1 {
            body.push(0);
        }
    }
    let mut out = b"LIST".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

fn junk(total: usize) -> Vec<u8> {
    let mut out = b"JUNK".to_vec();
    out.extend_from_slice(&((total - 8) as u32).to_le_bytes());
    out.resize(total, 0);
    out
}

/// 変更前後の INFO と、ヘッダー領域に残すチャンク
struct Header {
    region: Vec<u8>,
    kept: Vec<Vec<u8>>,
    /// INFO を入れる位置 (hdrl の直後)
    insert_at: usize,
    old_info: Vec<([u8; 4], Vec<u8>)>,
}

fn read_header(f: &mut File, lay: &Layout) -> Result<Header, String> {
    let region_len = lay.movi.off - 12;
    if region_len > MAX_REGION {
        return Err("AVI のヘッダーが大きすぎます".into());
    }
    let mut region = vec![0u8; region_len as usize];
    f.seek(SeekFrom::Start(12)).map_err(io)?;
    f.read_exact(&mut region).map_err(io)?;
    let mut kept = Vec::new();
    let mut insert_at = 0;
    let mut old_info = Vec::new();
    for (p, len) in region_chunks(&region)? {
        let c = &region[p..p + len];
        let id = &c[0..4];
        if id == b"LIST" && len >= 12 && &c[8..12] == b"INFO" {
            old_info.extend(parse_info(c));
        } else if id == b"JUNK" || id == b"junk" {
            // 空き領域は並べ直すときに作り直す
        } else {
            let mut v = c.to_vec();
            v.resize(len + (len & 1), 0);
            if id == b"LIST" && len >= 12 && &c[8..12] == b"hdrl" {
                insert_at = kept.len() + 1;
            }
            kept.push(v);
        }
    }
    Ok(Header { region, kept, insert_at, old_info })
}

/// 読み取り時の文字コード判定用: INFO の値に ASCII 以外があり、すべて正しい UTF-8 か
pub fn info_is_utf8(path: &Path) -> bool {
    let Ok(mut f) = File::open(path) else { return false };
    let Ok(lay) = parse(&mut f) else { return false };
    let Ok(h) = read_header(&mut f, &lay) else { return false };
    let values = || h.old_info.iter().map(|(_, v)| v);
    values().any(|v| !v.is_ascii()) && values().all(|v| std::str::from_utf8(v).is_ok())
}

/// 書き込み。変更が無ければ Ok(false) で、dst は作らない
pub fn write(src: &Path, dst: &Path, edits: &[Edit], remove_all: bool) -> Result<bool, String> {
    let mut f = File::open(src).map_err(io)?;
    let lay = parse(&mut f)?;
    let mut h = read_header(&mut f, &lay)?;

    // ExifTool の CharsetRIFF=Latin は Windows-1252。読み取りと同じく
    // INFO 全体で判定し、保持する値と新しい値を同じ文字コードにする。
    let mut original = std::mem::take(&mut h.old_info);
    if !remove_all && original.iter().any(|(_, v)| std::str::from_utf8(v).is_err()) {
        const C1: [char; 32] = [
            '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡',
            'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}',
            '\u{90}', '‘', '’', '“', '”', '•', '–', '—',
            '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
        ];
        for (_, value) in &mut original {
            let decoded: String = value.iter().map(|&b| match b {
                0x80..=0x9f => C1[(b - 0x80) as usize],
                _ => char::from(b),
            }).collect();
            *value = decoded.into_bytes();
        }
    }
    let mut info = if remove_all { Vec::new() } else { original.clone() };
    for e in edits {
        match &e.value {
            Some(v) => match info.iter().position(|(id, _)| *id == e.id) {
                Some(i) => {
                    info[i].1 = v.clone();
                    let mut seen = 0;
                    info.retain(|(id, _)| *id != e.id || { seen += 1; seen == 1 });
                }
                None => info.push((e.id, v.clone())),
            },
            None => info.retain(|(id, _)| *id != e.id),
        }
    }
    if info == original {
        return Ok(false);
    }

    let region_len = h.region.len();
    let fixed: usize = h.kept.iter().map(Vec::len).sum();
    let info_len = if info.is_empty() { 0 } else { build_info(&info, 0).len() };
    let used = fixed + info_len;

    // 1. ヘッダー領域の中に収める
    if used <= region_len {
        let rem = region_len - used;
        let fits = rem == 0 || rem >= 8 || (!info.is_empty() && rem % 2 == 0);
        if fits {
            let region = assemble(&h, &info, region_len);
            std::fs::copy(src, dst).map_err(|e| format!("作業用のコピーを作れませんでした: {e}"))?;
            let mut out = std::fs::OpenOptions::new().write(true).open(dst).map_err(io)?;
            out.seek(SeekFrom::Start(12)).map_err(io)?;
            out.write_all(&region).map_err(io)?;
            out.sync_all().map_err(io)?;
            return Ok(true);
        }
    }

    // 2. ヘッダー領域を広げて書き直す
    let need = (used + 8) as u64 + GROW_SLACK;
    let delta = (need - region_len as u64).div_ceil(GROW_ALIGN) * GROW_ALIGN;
    let riff_size = lay.riff_size as u64 + delta;
    if riff_size > u32::MAX as u64 {
        return Err("ファイルが大きすぎるため、タグを書き込めません".into());
    }
    let mut ix_positions = Vec::new();
    for c in h.kept.iter_mut() {
        if &c[0..4] == b"LIST" && &c[8..12] == b"hdrl" {
            ix_positions.extend(shift_super_index(c, delta, lay.movi.off)?);
        }
    }
    let region = assemble(&h, &info, region_len + delta as usize);
    {
        let mut out = BufWriter::new(File::create(dst).map_err(io)?);
        out.write_all(b"RIFF").map_err(io)?;
        out.write_all(&(riff_size as u32).to_le_bytes()).map_err(io)?;
        out.write_all(b"AVI ").map_err(io)?;
        out.write_all(&region).map_err(io)?;
        copy_range(&mut f, &mut out, lay.movi.off, lay.file_len - lay.movi.off)?;
        out.flush().map_err(io)?;
    }
    let mut out = std::fs::OpenOptions::new().read(true).write(true).open(dst).map_err(io)?;
    for pos in ix_positions {
        shift_chunk_index(&mut out, pos + delta, delta)?;
    }
    if let Some(idx1) = &lay.idx1 {
        shift_idx1(&mut out, idx1, &lay.movi, delta)?;
    }
    out.sync_all().map_err(io)?;
    Ok(true)
}

/// 残すチャンク + INFO + JUNK で、長さ len のヘッダー領域を作る
fn assemble(h: &Header, info: &[([u8; 4], Vec<u8>)], len: usize) -> Vec<u8> {
    let fixed: usize = h.kept.iter().map(Vec::len).sum();
    let mut info_chunk = if info.is_empty() { Vec::new() } else { build_info(info, 0) };
    let rem = len - fixed - info_chunk.len();
    let mut tail = Vec::new();
    if rem >= 8 {
        tail = junk(rem);
    } else if rem > 0 {
        info_chunk = build_info(info, rem);
    }
    let mut out = Vec::with_capacity(len);
    for (i, c) in h.kept.iter().enumerate() {
        if i == h.insert_at {
            out.extend_from_slice(&info_chunk);
        }
        out.extend_from_slice(c);
    }
    if h.insert_at >= h.kept.len() {
        out.extend_from_slice(&info_chunk);
    }
    out.extend(tail);
    debug_assert_eq!(out.len(), len);
    out
}

/// hdrl の中の OpenDML スーパー索引 (indx) の絶対位置をずらす。
/// 返り値は、ずらす前の標準索引 (ix##) の位置
fn shift_super_index(hdrl: &mut [u8], delta: u64, movi_off: u64) -> Result<Vec<u64>, String> {
    let mut ix = Vec::new();
    for (p, len) in region_chunks(&hdrl[12..])? {
        let p = p + 12;
        if &hdrl[p..p + 4] != b"LIST" || len < 12 || &hdrl[p + 8..p + 12] != b"strl" {
            continue;
        }
        for (q, qlen) in region_chunks(&hdrl[p + 12..p + len])? {
            let q = p + 12 + q;
            if &hdrl[q..q + 4] != b"indx" || qlen < 8 + 24 {
                continue;
            }
            let d = q + 8;
            let longs = u16::from_le_bytes([hdrl[d], hdrl[d + 1]]) as usize;
            let index_type = hdrl[d + 3];
            let entries = u32le(&hdrl[d + 4..d + 8]) as usize;
            match index_type {
                // AVI_INDEX_OF_INDEXES: 各エントリの先頭 8 バイトが ix## の絶対位置
                0 => {
                    let step = (longs * 4).max(16);
                    for i in 0..entries {
                        let e = d + 24 + i * step;
                        if e + 8 > q + qlen {
                            return Err("AVI の索引 (indx) が壊れています".into());
                        }
                        let old = u64le(&hdrl[e..e + 8]);
                        if old == 0 {
                            continue;
                        }
                        if old < movi_off {
                            return Err("AVI の索引の位置が想定外のため、書き込めません".into());
                        }
                        ix.push(old);
                        hdrl[e..e + 8].copy_from_slice(&(old + delta).to_le_bytes());
                    }
                }
                // AVI_INDEX_OF_CHUNKS: 基準位置 qwBaseOffset をずらす
                1 => {
                    let b = d + 12;
                    let old = u64le(&hdrl[b..b + 8]);
                    hdrl[b..b + 8].copy_from_slice(&(old + delta).to_le_bytes());
                }
                _ => return Err("対応していない AVI の索引形式です".into()),
            }
        }
    }
    Ok(ix)
}

/// 標準索引 (ix##) の qwBaseOffset をずらす
fn shift_chunk_index(f: &mut File, pos: u64, delta: u64) -> Result<(), String> {
    let mut h = [0u8; 28];
    f.seek(SeekFrom::Start(pos)).map_err(io)?;
    f.read_exact(&mut h).map_err(io)?;
    if h[11] != 1 {
        return Err("AVI の索引 (ix) が見つかりません".into());
    }
    let base = u64le(&h[20..28]) + delta;
    f.seek(SeekFrom::Start(pos + 20)).map_err(io)?;
    f.write_all(&base.to_le_bytes()).map_err(io)
}

/// idx1 が絶対位置で書かれている場合だけ、全エントリをずらす
fn shift_idx1(f: &mut File, idx1: &Chunk, movi: &Chunk, delta: u64) -> Result<(), String> {
    let pos = idx1.off + delta + 8;
    let mut data = vec![0u8; idx1.size as usize / 16 * 16];
    if data.is_empty() {
        return Ok(());
    }
    f.seek(SeekFrom::Start(pos)).map_err(io)?;
    f.read_exact(&mut data).map_err(io)?;
    // 相対位置なら 'movi' の 4 文字の位置からの距離 (最初は通常 4)。それ以上大きければ絶対位置
    let first = u32le(&data[8..12]) as u64;
    if first < movi.off + 8 {
        return Ok(());
    }
    for e in data.chunks_exact_mut(16) {
        let v = u32le(&e[8..12]) as u64 + delta;
        let v = u32::try_from(v).map_err(|_| "AVI の索引を直せませんでした")?;
        e[8..12].copy_from_slice(&v.to_le_bytes());
    }
    f.seek(SeekFrom::Start(pos)).map_err(io)?;
    f.write_all(&data).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_info_roundtrips_with_japanese_edit() {
        let mut body = b"AVI ".to_vec();
        body.extend(build_info(&[(*b"IART", b"Andr\xe9 \x80".to_vec())], 0));
        body.extend_from_slice(b"LIST\x04\x00\x00\x00movi");
        let file = [b"RIFF".as_slice(), &(body.len() as u32).to_le_bytes(), &body].concat();
        super::super::with_fixture("avi", &file, |path| {
            let edit = TagChange { group: "RIFF".into(), name: "Title".into(), value: Some("日本語".into()) };
            let result = super::super::apply(path.to_str().unwrap(), &[edit], false, false).unwrap();
            assert!(result.updated, "{:?}", result.errors);
            assert!(info_is_utf8(path));
            let mut f = File::open(path).unwrap();
            let layout = parse(&mut f).unwrap();
            let info = read_header(&mut f, &layout).unwrap().old_info;
            assert_eq!(info, vec![(*b"IART", "André €".as_bytes().to_vec()), (*b"INAM", "日本語".as_bytes().to_vec())]);
        });
    }

    #[test]
    fn info_roundtrip() {
        let items = vec![(*b"INAM", "タイトル".as_bytes().to_vec()), (*b"IART", b"abc".to_vec())];
        let c = build_info(&items, 0);
        assert_eq!(c.len() % 2, 0);
        assert_eq!(parse_info(&c), items);
        let padded = build_info(&items, 4);
        assert_eq!(padded.len(), c.len() + 4);
        assert_eq!(parse_info(&padded), items);
    }

    #[test]
    fn plan_maps_names() {
        let ch = |g: &str, n: &str, v: Option<&str>| TagChange { group: g.into(), name: n.into(), value: v.map(String::from) };
        let (e, w) = plan(&[
            ch("RIFF", "Title", Some("t")),
            ch("RIFF", "DateCreated", Some("2024:05:01 10:20:30")),
            ch("", "artist", Some("")),
            ch("RIFF", "VideoCodec", Some("x")),
        ]);
        assert_eq!(e.len(), 3);
        assert_eq!(e[1].value.as_deref(), Some(&b"2024-05-01 10:20:30"[..]));
        assert_eq!(e[2].id, *b"IART");
        assert!(e[2].value.is_none());
        assert_eq!(w.len(), 1);
    }
}
