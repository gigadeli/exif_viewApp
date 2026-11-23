//! EBML (Matroska の土台) の可変長整数と要素の読み書き。

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

pub const VOID: u32 = 0xEC;
pub const CRC32: u32 = 0xBF;

/// 要素 ID を読む (先頭のマーカーのビットも含めた値)。返り値は (ID, 長さ)
pub fn read_id(b: &[u8]) -> Option<(u32, usize)> {
    let first = *b.first()?;
    let len = first.leading_zeros() as usize + 1;
    if len > 4 || b.len() < len {
        return None;
    }
    Some((b[..len].iter().fold(0u32, |a, &x| (a << 8) | x as u32), len))
}

/// サイズを読む。すべて 1 の値は「不明」(None)。返り値は (サイズ, 長さ)
pub fn read_size(b: &[u8]) -> Option<(Option<u64>, usize)> {
    let first = *b.first()?;
    let len = first.leading_zeros() as usize + 1;
    if len > 8 || b.len() < len {
        return None;
    }
    let mask = if len == 8 { 0 } else { 0xFFu8 >> len };
    let mut v = (first & mask) as u64;
    for &x in &b[1..len] {
        v = (v << 8) | x as u64;
    }
    let unknown = v == (1u64 << (7 * len)) - 1;
    Some((if unknown { None } else { Some(v) }, len))
}

pub fn id_bytes(id: u32) -> Vec<u8> {
    let b = id.to_be_bytes();
    let skip = b.iter().take_while(|&&x| x == 0).count().min(3);
    b[skip..].to_vec()
}

/// サイズ n を書くのに必要な最小の長さ (すべて 1 は「不明」なので使えない)
pub fn size_len_needed(n: u64) -> usize {
    (1..=8).find(|&l| n < (1u64 << (7 * l)) - 1).unwrap_or(8)
}

/// サイズを長さ len (以上) で書く
pub fn size_bytes(n: u64, len: usize) -> Vec<u8> {
    let len = len.max(size_len_needed(n)).min(8);
    let v = n | (1u64 << (7 * len));
    v.to_be_bytes()[8 - len..].to_vec()
}

/// 要素を組み立てる。size_len はサイズ欄の最小の長さ
pub fn element_with(id: u32, body: &[u8], size_len: usize) -> Vec<u8> {
    let mut out = id_bytes(id);
    out.extend(size_bytes(body.len() as u64, size_len));
    out.extend_from_slice(body);
    out
}

pub fn element(id: u32, body: &[u8]) -> Vec<u8> {
    element_with(id, body, 1)
}

pub fn uint_bytes(v: u64, min_len: usize) -> Vec<u8> {
    let needed = (8 - v.leading_zeros() as usize / 8).max(1);
    v.to_be_bytes()[8 - needed.max(min_len).min(8)..].to_vec()
}

pub fn uint(b: &[u8]) -> u64 {
    b.iter().take(8).fold(0u64, |a, &x| (a << 8) | x as u64)
}

/// 長さ total の Void 要素 (total は 2 以上)
pub fn void(total: usize) -> Option<Vec<u8>> {
    for len in 1..=8usize {
        let Some(data) = total.checked_sub(1 + len) else { return None };
        if (data as u64) < (1u64 << (7 * len)) - 1 {
            let mut out = vec![VOID as u8];
            out.extend(size_bytes(data as u64, len));
            debug_assert_eq!(out.len(), 1 + len);
            out.resize(total, 0);
            return Some(out);
        }
    }
    None
}

/// 要素を、長さ space の領域にちょうど収める (余りは Void、1 バイト余ればサイズ欄を伸ばす)
pub fn fit(id: u32, body: &[u8], size_len: usize, space: usize) -> Option<Vec<u8>> {
    let mut e = element_with(id, body, size_len);
    if e.len() > space {
        return None;
    }
    let rem = space - e.len();
    if rem == 1 {
        let cur = size_len.max(size_len_needed(body.len() as u64));
        if cur >= 8 {
            return None;
        }
        e = element_with(id, body, cur + 1);
    } else if rem >= 2 {
        e.extend(void(rem)?);
    }
    (e.len() == space).then_some(e)
}

/// 子要素 (メモリ上)
#[derive(Debug, Clone)]
pub struct Child<'a> {
    pub id: u32,
    pub size_len: usize,
    pub body: &'a [u8],
}

impl Child<'_> {
    pub fn encode(&self) -> Vec<u8> {
        element_with(self.id, self.body, self.size_len)
    }
}

pub fn children(b: &[u8]) -> Result<Vec<Child<'_>>, String> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < b.len() {
        let bad = || "Matroska の要素が壊れています".to_string();
        let (id, il) = read_id(&b[p..]).ok_or_else(bad)?;
        let (size, sl) = read_size(&b[p + il..]).ok_or_else(bad)?;
        let size = size.ok_or_else(bad)? as usize;
        let start = p + il + sl;
        if start + size > b.len() {
            return Err(bad());
        }
        out.push(Child { id, size_len: sl, body: &b[start..start + size] });
        p = start + size;
    }
    Ok(out)
}

/// 最初の子が CRC-32 なら取り除いて (true, 残り) を返す
pub fn strip_crc(body: &[u8]) -> (bool, &[u8]) {
    match children_prefix(body) {
        Some((CRC32, end)) => (true, &body[end..]),
        _ => (false, body),
    }
}

fn children_prefix(body: &[u8]) -> Option<(u32, usize)> {
    let (id, il) = read_id(body)?;
    let (size, sl) = read_size(&body[il..])?;
    Some((id, il + sl + size? as usize))
}

/// CRC-32 を付け直す (had_crc のときだけ)
pub fn with_crc(had_crc: bool, rest: Vec<u8>) -> Vec<u8> {
    if !had_crc {
        return rest;
    }
    let crc = crc32fast::hash(&rest);
    let mut out = vec![CRC32 as u8, 0x84];
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend(rest);
    out
}

/// ファイル上の要素の見出し
#[derive(Debug, Clone, Copy)]
pub struct Header {
    pub id: u32,
    /// 見出し (ID + サイズ欄) の長さ
    pub hlen: usize,
    pub size: Option<u64>,
    pub size_len: usize,
}

pub fn read_header(f: &mut File, pos: u64) -> Result<Option<Header>, String> {
    let mut b = [0u8; 12];
    f.seek(SeekFrom::Start(pos)).map_err(|e| e.to_string())?;
    let mut n = 0;
    while n < b.len() {
        let r = f.read(&mut b[n..]).map_err(|e| e.to_string())?;
        if r == 0 {
            break;
        }
        n += r;
    }
    let Some((id, il)) = read_id(&b[..n]) else { return Ok(None) };
    let Some((size, sl)) = read_size(&b[il..n]) else { return Ok(None) };
    Ok(Some(Header { id, hlen: il + sl, size, size_len: sl }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vints() {
        assert_eq!(read_id(&[0x1A, 0x45, 0xDF, 0xA3]), Some((0x1A45DFA3, 4)));
        assert_eq!(read_id(&[0xEC]), Some((0xEC, 1)));
        assert_eq!(id_bytes(0x1254C367), vec![0x12, 0x54, 0xC3, 0x67]);
        assert_eq!(id_bytes(0xEC), vec![0xEC]);
        assert_eq!(read_size(&[0x81]), Some((Some(1), 1)));
        assert_eq!(read_size(&[0xFF]), Some((None, 1)));
        assert_eq!(read_size(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]), Some((None, 8)));
        assert_eq!(size_bytes(126, 1), vec![0xFE]);
        assert_eq!(size_bytes(127, 1), vec![0x40, 0x7F]);
        assert_eq!(size_bytes(5, 8), vec![1, 0, 0, 0, 0, 0, 0, 5]);
        for n in [0u64, 1, 126, 127, 16382, 16383, 1 << 40] {
            for l in 1..=8 {
                let b = size_bytes(n, l);
                assert_eq!(read_size(&b), Some((Some(n), b.len())), "{n} {l}");
            }
        }
        assert_eq!(uint_bytes(0, 1), vec![0]);
        assert_eq!(uint_bytes(300, 1), vec![1, 44]);
        assert_eq!(uint_bytes(300, 4), vec![0, 0, 1, 44]);
        assert_eq!(uint(&[1, 44]), 300);
    }

    #[test]
    fn voids_and_fit() {
        for total in 2..300 {
            let v = void(total).unwrap();
            assert_eq!(v.len(), total);
            let c = children(&v).unwrap();
            assert_eq!(c.len(), 1);
            assert_eq!(c[0].id, VOID);
        }
        assert!(void(1).is_none());
        let body = b"abc";
        for space in 6..40 {
            let e = fit(0x7BA9, body, 1, space).unwrap();
            assert_eq!(e.len(), space);
            let c = children(&e).unwrap();
            assert_eq!(c[0].id, 0x7BA9);
            assert_eq!(c[0].body, body);
        }
        assert!(fit(0x7BA9, body, 1, 5).is_none());
    }

    #[test]
    fn crc() {
        let rest = element(0x7BA9, b"title");
        let body = with_crc(true, rest.clone());
        let (had, r) = strip_crc(&body);
        assert!(had);
        assert_eq!(r, &rest[..]);
        let (had, r) = strip_crc(&rest);
        assert!(!had);
        assert_eq!(r, &rest[..]);
    }
}
