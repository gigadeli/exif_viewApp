//! MKV / WebM (Matroska) の Info と Tags。設計は docs/native-writer.md の 4 章。
//!
//! 最初の Cluster より前の「ヘッダー領域」の中で要素を並べ直し、余りを Void にする。
//! Cluster 以降 (映像・音声データと Cues) は動かさない。

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::dash_date;
use super::ebml::{self, children, element, element_with, fit, read_header, strip_crc, uint, uint_bytes, with_crc};
use crate::write::TagChange;

const EBML_HEADER: u32 = 0x1A45DFA3;
const DOCTYPE: u32 = 0x4282;
const SEGMENT: u32 = 0x18538067;
const SEEKHEAD: u32 = 0x114D9B74;
const SEEK: u32 = 0x4DBB;
const SEEK_ID: u32 = 0x53AB;
const SEEK_POSITION: u32 = 0x53AC;
const INFO: u32 = 0x1549A966;
const TITLE: u32 = 0x7BA9;
const DATE_UTC: u32 = 0x4461;
const TRACKS: u32 = 0x1654AE6B;
const CUES: u32 = 0x1C53BB6B;
const TAGS: u32 = 0x1254C367;
const TAG: u32 = 0x7373;
const TARGETS: u32 = 0x63C0;
const TARGET_TYPE_VALUE: u32 = 0x68CA;
const TARGET_UIDS: [u32; 4] = [0x63C5, 0x63C9, 0x63C4, 0x63C6];
const SIMPLE_TAG: u32 = 0x67C8;
const TAG_NAME: u32 = 0x45A3;
const TAG_STRING: u32 = 0x4487;
const TAG_BINARY: u32 = 0x4485;
const CLUSTER: u32 = 0x1F43B675;
const ATTACHMENTS: u32 = 0x1941A469;
const CHAPTERS: u32 = 0x1043A770;
const VOID: u32 = ebml::VOID;

/// Segment の直下に置かれる要素 (サイズ不明の Cluster の終わりを見つけるのに使う)
const LEVEL1: [u32; 9] = [SEEKHEAD, INFO, TRACKS, CUES, TAGS, CLUSTER, ATTACHMENTS, CHAPTERS, SEGMENT];

/// Tags をメモリに読み込む上限
const MAX_ELEMENT: u64 = 64 * 1024 * 1024;
/// ヘッダー領域として読み込む上限 (添付ファイルが大きい場合など)
const MAX_HEADER: u64 = 512 * 1024 * 1024;

/// ヘッダー領域を広げるときに、次回のために残す余白
const GROW_SLACK: u64 = 4096;
const CUE_POINT: u32 = 0xBB;
const CUE_TRACK_POSITIONS: u32 = 0xB7;
const CUE_CLUSTER_POSITION: u32 = 0xF1;
const CUE_CODEC_STATE: u32 = 0xEA;
const CLUSTER_POSITION: u32 = 0xA7;

/// グループ Info で書けるタグ
pub const INFO_NAMES: [&str; 2] = ["Title", "DateTimeOriginal"];

/// ExifTool のタグ名 (グループ Matroska) と SimpleTag の TagName (ExifTool の StdTag 表の逆引き)
pub const SIMPLE_TAGS: &[(&str, &str)] = &[
    ("Title", "TITLE"),
    ("Subtitle", "SUBTITLE"),
    ("Artist", "ARTIST"),
    ("LeadPerformer", "LEAD_PERFORMER"),
    ("Accompaniment", "ACCOMPANIMENT"),
    ("Composer", "COMPOSER"),
    ("Arranger", "ARRANGER"),
    ("Lyrics", "LYRICS"),
    ("Lyricist", "LYRICIST"),
    ("Conductor", "CONDUCTOR"),
    ("Director", "DIRECTOR"),
    ("AssistantDirector", "ASSISTANT_DIRECTOR"),
    ("DirectorOfPhotography", "DIRECTOR_OF_PHOTOGRAPHY"),
    ("SoundEngineer", "SOUND_ENGINEER"),
    ("ArtDirector", "ART_DIRECTOR"),
    ("ProductionDesigner", "PRODUCTION_DESIGNER"),
    ("CostumeDesigner", "COSTUME_DESIGNER"),
    ("Actor", "ACTOR"),
    ("Character", "CHARACTER"),
    ("WrittenBy", "WRITTEN_BY"),
    ("ScreenplayBy", "SCREENPLAY_BY"),
    ("EditedBy", "EDITED_BY"),
    ("Producer", "PRODUCER"),
    ("Coproducer", "COPRODUCER"),
    ("ExecutiveProducer", "EXECUTIVE_PRODUCER"),
    ("DistributedBy", "DISTRIBUTED_BY"),
    ("MasteredBy", "MASTERED_BY"),
    ("EncodedBy", "ENCODED_BY"),
    ("MixedBy", "MIXED_BY"),
    ("ProductionStudio", "PRODUCTION_STUDIO"),
    ("ThanksTo", "THANKS_TO"),
    ("Publisher", "PUBLISHER"),
    ("Label", "LABEL"),
    ("Genre", "GENRE"),
    ("Mood", "MOOD"),
    ("OriginalMediaType", "ORIGINAL_MEDIA_TYPE"),
    ("ContentType", "CONTENT_TYPE"),
    ("Subject", "SUBJECT"),
    ("Description", "DESCRIPTION"),
    ("Keywords", "KEYWORDS"),
    ("Summary", "SUMMARY"),
    ("Synopsis", "SYNOPSIS"),
    ("Period", "PERIOD"),
    ("LawRating", "LAW_RATING"),
    ("DateReleased", "DATE_RELEASED"),
    ("DateTimeOriginal", "DATE_RECORDED"),
    ("DateEncoded", "DATE_ENCODED"),
    ("DateTagged", "DATE_TAGGED"),
    ("CreateDate", "DATE_DIGITIZED"),
    ("DateWritten", "DATE_WRITTEN"),
    ("RecordingLocation", "RECORDING_LOCATION"),
    ("CompositionLocation", "COMPOSITION_LOCATION"),
    ("Comment", "COMMENT"),
    ("Rating", "RATING"),
    ("Encoder", "ENCODER"),
    ("EncoderSettings", "ENCODER_SETTINGS"),
    ("URL", "URL"),
    ("Email", "EMAIL"),
    ("ISRC", "ISRC"),
    ("ISBN", "ISBN"),
    ("Barcode", "BARCODE"),
    ("CatalogNumber", "CATALOG_NUMBER"),
    ("IMDB", "IMDB"),
    ("TMDB", "TMDB"),
    ("TVDB", "TVDB"),
    ("Copyright", "COPYRIGHT"),
    ("ProductionCopyright", "PRODUCTION_COPYRIGHT"),
    ("License", "LICENSE"),
    ("TermsOfUse", "TERMS_OF_USE"),
    ("PartNumber", "PART_NUMBER"),
    ("TotalParts", "TOTAL_PARTS"),
];

#[derive(Debug, Clone)]
pub enum Edit {
    Title(Option<String>),
    /// 2001-01-01T00:00:00Z からのナノ秒
    DateUtc(Option<i64>),
    Simple { name: &'static str, value: Option<String> },
}

fn io(e: std::io::Error) -> String {
    e.to_string()
}

/// EBML ヘッダーの DocType が matroska / webm か
pub fn is_matroska(path: &Path) -> bool {
    let Ok(mut f) = File::open(path) else { return false };
    doc_type(&mut f).is_ok_and(|t| t == "matroska" || t == "webm")
}

fn doc_type(f: &mut File) -> Result<String, String> {
    let h = read_header(f, 0)?.filter(|h| h.id == EBML_HEADER).ok_or("Matroska ではありません")?;
    let size = h.size.filter(|&s| s < 4096).ok_or("EBML ヘッダーが壊れています")?;
    let mut body = vec![0u8; size as usize];
    f.seek(SeekFrom::Start(h.hlen as u64)).map_err(io)?;
    f.read_exact(&mut body).map_err(io)?;
    let c = children(&body)?;
    let t = c.iter().find(|c| c.id == DOCTYPE).ok_or("DocType がありません")?;
    Ok(String::from_utf8_lossy(t.body).trim_end_matches('\0').to_string())
}

/// "YYYY:MM:DD HH:MM:SS[.fff][Z|+hh:mm]" を 2001-01-01 からの UTC のナノ秒にする (タイムゾーンなしは UTC)
pub fn parse_date(s: &str) -> Option<i64> {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (num(0..4)?, num(5..7)?, num(8..10)?, num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let mut rest = &s[19..];
    let mut nanos = 0i64;
    if let Some(r) = rest.strip_prefix('.') {
        let digits: String = r.chars().take_while(char::is_ascii_digit).collect();
        rest = &r[digits.len()..];
        let mut d9 = digits.clone();
        d9.truncate(9);
        while d9.len() < 9 {
            d9.push('0');
        }
        nanos = d9.parse().ok()?;
    }
    let offset = match rest.trim() {
        "" | "Z" => 0,
        t => {
            let sign = match t.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let hh: i64 = t.get(1..3)?.parse().ok()?;
            let mm: i64 = t.get(4..6).map(|m| m.parse().ok()).unwrap_or(Some(0))?;
            sign * (hh * 3600 + mm * 60)
        }
    };
    // 日付 → 1970-01-01 からの日数 (Howard Hinnant の days_from_civil)
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let unix = days * 86400 + h * 3600 + mi * 60 + se - offset;
    let timestamp = i128::from(unix - 978_307_200) * 1_000_000_000 + i128::from(nanos);
    i64::try_from(timestamp).ok()
}

/// タグの変更を Matroska の変更にする。書けないタグは警告の文にして返す
pub fn plan(changes: &[TagChange]) -> Result<(Vec<Edit>, Vec<String>), String> {
    let mut edits = Vec::new();
    let mut warn = Vec::new();
    for c in changes {
        if c.group.is_empty() && c.name.eq_ignore_ascii_case("all") && c.value.is_none() {
            continue;
        }
        let value = c.value.clone().filter(|v| !v.is_empty());
        let info = c.group == "Info" || (c.group.is_empty() && INFO_NAMES.iter().any(|n| n.eq_ignore_ascii_case(&c.name)));
        if info && c.name.eq_ignore_ascii_case("Title") {
            edits.push(Edit::Title(value));
            continue;
        }
        if info && c.name.eq_ignore_ascii_case("DateTimeOriginal") {
            match value.as_deref().map(parse_date) {
                None => edits.push(Edit::DateUtc(None)),
                Some(Some(ns)) => edits.push(Edit::DateUtc(Some(ns))),
                Some(None) => return Err(format!("「{}」は Matroska の日時として保存できません", c.value.as_deref().unwrap_or(""))),
            }
            continue;
        }
        let simple = (c.group.is_empty() || c.group == "Matroska")
            .then(|| SIMPLE_TAGS.iter().find(|(n, _)| n.eq_ignore_ascii_case(&c.name)))
            .flatten();
        match simple {
            Some((n, tag)) => {
                let is_date = n.starts_with("Date") || *n == "CreateDate";
                let value = value.map(|v| if is_date { dash_date(&v) } else { v });
                edits.push(Edit::Simple { name: tag, value });
            }
            None => warn.push(format!("Warning: 「{}:{}」は MKV / WebM に書き込めないタグです", c.group, c.name)),
        }
    }
    Ok((edits, warn))
}

// ---------- 要素の中身の書き換え ----------

/// CRC-32 を外して中身を書き換え、付け直す
fn edit_body(body: &[u8], f: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>) -> Result<Vec<u8>, String> {
    let (crc, rest) = strip_crc(body);
    Ok(with_crc(crc, f(rest)?))
}

fn edit_info(body: &[u8], edits: &[Edit], remove_all: bool) -> Result<Vec<u8>, String> {
    edit_body(body, |rest| {
        let mut title: Option<Option<Vec<u8>>> = None;
        let mut date: Option<Option<Vec<u8>>> = None;
        if remove_all {
            title = Some(None);
            date = Some(None);
        }
        for e in edits {
            match e {
                Edit::Title(v) => title = Some(v.as_ref().map(|s| s.as_bytes().to_vec())),
                Edit::DateUtc(v) => date = Some(v.map(|ns| ns.to_be_bytes().to_vec())),
                Edit::Simple { .. } => {}
            }
        }
        let mut out = Vec::new();
        let (mut has_title, mut has_date) = (false, false);
        for c in children(rest)? {
            match (c.id, &title, &date) {
                (TITLE, Some(v), _) => {
                    if let Some(v) = v.as_ref().filter(|_| !has_title) {
                        out.extend(element(TITLE, v));
                    }
                    has_title = true;
                }
                (DATE_UTC, _, Some(v)) => {
                    if let Some(v) = v.as_ref().filter(|_| !has_date) {
                        out.extend(element(DATE_UTC, v));
                    }
                    has_date = true;
                }
                (VOID, _, _) => {}
                _ => out.extend(c.encode()),
            }
        }
        if let (false, Some(Some(v))) = (has_title, &title) {
            out.extend(element(TITLE, v));
        }
        if let (false, Some(Some(v))) = (has_date, &date) {
            out.extend(element(DATE_UTC, v));
        }
        Ok(out)
    })
}

/// Tag の Targets を見て (全体向けか, TargetTypeValue) を返す
fn tag_target(tag_rest: &[u8]) -> Result<(bool, u64), String> {
    for c in children(tag_rest)? {
        if c.id == TARGETS {
            let mut global = true;
            let mut ttv = 50;
            for t in children(c.body)? {
                if TARGET_UIDS.contains(&t.id) && uint(t.body) != 0 {
                    global = false;
                }
                if t.id == TARGET_TYPE_VALUE {
                    ttv = uint(t.body);
                }
            }
            return Ok((global, ttv));
        }
    }
    Ok((true, 50))
}

fn simple_name(st: &[u8]) -> Result<Option<String>, String> {
    Ok(children(st)?
        .iter()
        .find(|c| c.id == TAG_NAME)
        .map(|c| String::from_utf8_lossy(c.body).trim_end_matches('\0').to_string()))
}

fn new_simple(name: &str, value: &str) -> Vec<u8> {
    let mut body = element(TAG_NAME, name.as_bytes());
    body.extend(element(TAG_STRING, value.as_bytes()));
    element(SIMPLE_TAG, &body)
}

/// SimpleTag の値を置き換える (言語などの属性と、入れ子の SimpleTag は残す)
fn replace_simple_value(st: &[u8], value: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut done = false;
    for c in children(st)? {
        match c.id {
            TAG_STRING | TAG_BINARY => {
                if !done {
                    out.extend(element(TAG_STRING, value.as_bytes()));
                    done = true;
                }
            }
            _ => out.extend(c.encode()),
        }
    }
    if !done {
        out.extend(element(TAG_STRING, value.as_bytes()));
    }
    Ok(out)
}

/// 1 つの Tags 要素の中の全体向け Tag に変更を当てる。
/// set 済み (見つかって書き換えた) の名前を done に入れる
fn edit_tags(body: &[u8], edits: &[(&'static str, Option<String>)], done: &mut Vec<&'static str>) -> Result<Vec<u8>, String> {
    edit_body(body, |rest| {
        let mut out = Vec::new();
        for c in children(rest)? {
            if c.id == VOID {
                continue;
            }
            if c.id != TAG {
                out.extend(c.encode());
                continue;
            }
            let tag = edit_body(c.body, |tag_rest| {
                let (global, _) = tag_target(tag_rest)?;
                if !global {
                    return Ok(tag_rest.to_vec());
                }
                let mut t = Vec::new();
                for s in children(tag_rest)? {
                    if s.id != SIMPLE_TAG {
                        if s.id != VOID {
                            t.extend(s.encode());
                        }
                        continue;
                    }
                    let name = simple_name(s.body)?.unwrap_or_default();
                    match edits.iter().find(|(n, _)| n.eq_ignore_ascii_case(&name)) {
                        None => t.extend(s.encode()),
                        Some((n, value)) => {
                            // 編集要求には言語・対象の識別子がない。
                            if done.contains(n) {
                                return Err(format!("「{n}」が複数あり、編集対象を特定できません"));
                            }
                            if let Some(v) = value {
                                t.extend(element(SIMPLE_TAG, &replace_simple_value(s.body, v)?));
                            }
                            done.push(n);
                        }
                    }
                }
                Ok(t)
            })?;
            out.extend(element(TAG, &tag));
        }
        Ok(out)
    })
}

/// まだどこにも無かった SimpleTag を、全体向け (TargetTypeValue 50) の Tag に足す。足せたら true
fn add_to_global(body: &mut Vec<u8>, extra: &[u8]) -> Result<bool, String> {
    let mut added = false;
    let new = edit_body(body, |rest| {
        let mut out = Vec::new();
        for c in children(rest)? {
            if c.id == TAG && !added {
                let (crc, tag_rest) = strip_crc(c.body);
                let (global, ttv) = tag_target(tag_rest)?;
                if global && ttv == 50 {
                    let mut t = tag_rest.to_vec();
                    t.extend_from_slice(extra);
                    out.extend(element(TAG, &with_crc(crc, t)));
                    added = true;
                    continue;
                }
            }
            out.extend(c.encode());
        }
        Ok(out)
    })?;
    if added {
        *body = new;
    }
    Ok(added)
}

/// Targets が空 (= TargetTypeValue 50、ファイル全体) の Tag
fn new_global_tag(extra: &[u8]) -> Vec<u8> {
    let mut t = element(TARGETS, &[]);
    t.extend_from_slice(extra);
    element(TAG, &t)
}

// ---------- ファイルの構造 ----------

#[derive(Debug, Clone)]
struct L1 {
    id: u32,
    off: u64,
    hlen: usize,
    size_len: usize,
    size: u64,
}

impl L1 {
    fn total(&self) -> u64 {
        self.hlen as u64 + self.size
    }
    fn end(&self) -> u64 {
        self.off + self.total()
    }
}

struct Structure {
    file_len: u64,
    /// Segment のサイズ欄の位置と長さ (サイズ不明なら None)
    seg_size_field: Option<(u64, usize)>,
    seg_data: u64,
    seg_end: u64,
    /// Segment の直下の要素 (Void も含む)
    els: Vec<L1>,
    /// 最初の Cluster の位置 (ヘッダー領域の終わり)
    header_end: u64,
    /// Segment の終わりまで読めたか (末尾に追加してよいか)
    complete: bool,
}

fn parse(f: &mut File) -> Result<Structure, String> {
    let file_len = f.metadata().map_err(io)?.len();
    let ebml = read_header(f, 0)?.filter(|h| h.id == EBML_HEADER).ok_or("Matroska ではありません")?;
    let pos = ebml.hlen as u64 + ebml.size.ok_or("EBML ヘッダーが壊れています")?;
    let seg = read_header(f, pos)?.filter(|h| h.id == SEGMENT).ok_or("Segment が見つかりません")?;
    let seg_data = pos + seg.hlen as u64;
    let id_len = seg.hlen - seg.size_len;
    let seg_size_field = seg.size.map(|_| (pos + id_len as u64, seg.size_len));
    let mut seg_end = seg.size.map(|s| seg_data + s).unwrap_or(file_len);
    if seg_end > file_len {
        return Err("ファイルが途中で切れています".into());
    }
    let mut els = Vec::new();
    let mut header_end = None;
    let mut p = seg_data;
    let mut complete = true;
    while p < seg_end {
        let Some(h) = read_header(f, p)? else {
            complete = false;
            break;
        };
        if matches!(h.id, EBML_HEADER | SEGMENT) {
            if seg.size.is_some() {
                return Err("Segment の境界が壊れています".into());
            }
            seg_end = p;
            break;
        }
        let size = match h.size {
            Some(s) => s,
            None if h.id == CLUSTER => unknown_cluster_end(f, p + h.hlen as u64, seg_end)? - p - h.hlen as u64,
            None => {
                complete = false;
                break;
            }
        };
        let el = L1 { id: h.id, off: p, hlen: h.hlen, size_len: h.size_len, size };
        if el.end() > seg_end {
            complete = false;
            break;
        }
        if h.id == CLUSTER && header_end.is_none() {
            header_end = Some(p);
        }
        p = el.end();
        els.push(el);
    }
    let header_end = header_end.unwrap_or(p.min(seg_end));
    if !els.iter().any(|e| e.off >= header_end) && !complete {
        // ヘッダー領域の途中で読めなくなった
        return Err("MKV / WebM の構造を読み取れません".into());
    }
    Ok(Structure { file_len, seg_size_field, seg_data, seg_end, els, header_end, complete })
}

/// サイズ不明の Cluster の終わり (Segment 直下の要素が現れる位置) を探す
fn unknown_cluster_end(f: &mut File, mut p: u64, limit: u64) -> Result<u64, String> {
    while p < limit {
        let Some(h) = read_header(f, p)? else { return Ok(limit) };
        if LEVEL1.contains(&h.id) || h.id == EBML_HEADER {
            return Ok(p);
        }
        let size = h.size.ok_or("サイズ不明の要素が入れ子になっています")?;
        p += h.hlen as u64 + size;
    }
    Ok(limit.min(p))
}

fn read_body(f: &mut File, el: &L1) -> Result<Vec<u8>, String> {
    if el.size > MAX_ELEMENT {
        return Err("要素が大きすぎます".into());
    }
    let mut b = vec![0u8; el.size as usize];
    f.seek(SeekFrom::Start(el.off + el.hlen as u64)).map_err(io)?;
    f.read_exact(&mut b).map_err(io)?;
    Ok(b)
}

// ---------- SeekHead ----------

/// 位置の変化: 元の相対位置 → 新しい相対位置 (None は削除)
type PosMap = HashMap<u64, Option<u64>>;

fn rebuild_seekhead(body: &[u8], map: &PosMap, added: &[(u32, u64)]) -> Result<Vec<u8>, String> {
    edit_body(body, |rest| {
        let mut out = Vec::new();
        for c in children(rest)? {
            if c.id == VOID {
                continue;
            }
            if c.id != SEEK {
                out.extend(c.encode());
                continue;
            }
            let entries = children(c.body)?;
            let Some(pos_el) = entries.iter().find(|e| e.id == SEEK_POSITION) else {
                out.extend(c.encode());
                continue;
            };
            let old = uint(pos_el.body);
            let new = match map.get(&old) {
                None => old,
                Some(None) => continue,
                Some(Some(n)) => *n,
            };
            let mut s = Vec::new();
            for e in &entries {
                if e.id == SEEK_POSITION {
                    s.extend(element(SEEK_POSITION, &uint_bytes(new, pos_el.body.len())));
                } else {
                    s.extend(e.encode());
                }
            }
            out.extend(element_with(SEEK, &s, c.size_len));
        }
        for &(id, pos) in added {
            out.extend(seek_entry(id, pos));
        }
        Ok(out)
    })
}

fn seek_entry(id: u32, pos: u64) -> Vec<u8> {
    let mut s = element(SEEK_ID, &ebml::id_bytes(id));
    s.extend(element(SEEK_POSITION, &uint_bytes(pos, 4)));
    element(SEEK, &s)
}

// ---------- ヘッダー領域の並べ直し ----------

#[derive(Debug, Clone)]
struct Item {
    id: u32,
    /// 元の相対位置 (新しく作った要素は None)
    old_rel: Option<u64>,
    body: Vec<u8>,
    size_len: usize,
}

/// ヘッダー領域を長さ len に並べ直す。SeekHead の位置は落ち着くまで計算し直す。
/// 返り値は (領域のバイト列, 位置の変化)。入りきらなければ Err(必要な長さ)
fn pack(items: &[Item], len: usize, outside: &PosMap, added_outside: &[(u32, u64)]) -> Result<Result<(Vec<u8>, PosMap), usize>, String> {
    let originals: Vec<Vec<u8>> = items.iter().map(|i| i.body.clone()).collect();
    let mut bodies = originals.clone();
    for _ in 0..8 {
        // 今の大きさで位置を決める
        let mut map = outside.clone();
        let mut added = added_outside.to_vec();
        let mut p = 0u64;
        for (it, b) in items.iter().zip(&bodies) {
            match it.old_rel {
                Some(o) => {
                    if o != p {
                        map.insert(o, Some(p));
                    }
                }
                None if it.id != SEEKHEAD => added.push((it.id, p)),
                None => {}
            }
            p += element_with(it.id, b, it.size_len).len() as u64;
        }
        // SeekHead を作り直し、大きさが変わらなければ確定
        let mut changed = false;
        for (i, it) in items.iter().enumerate() {
            if it.id == SEEKHEAD {
                let nb = rebuild_seekhead(&originals[i], &map, &added)?;
                if nb.len() != bodies[i].len() {
                    changed = true;
                }
                bodies[i] = nb;
            }
        }
        if changed {
            continue;
        }
        let total = p as usize;
        if total > len {
            return Ok(Err(total));
        }
        let mut out = Vec::with_capacity(len);
        let rem = len - total;
        for (i, (it, b)) in items.iter().zip(&bodies).enumerate() {
            if rem == 1 && i + 1 == items.len() {
                // 余りが 1 バイトだけなら、最後の要素のサイズ欄を 1 バイト伸ばす
                let cur = it.size_len.max(ebml::size_len_needed(b.len() as u64));
                if cur >= 8 {
                    return Ok(Err(len + 2));
                }
                out.extend(element_with(it.id, b, cur + 1));
            } else {
                out.extend(element_with(it.id, b, it.size_len));
            }
        }
        if rem >= 2 {
            out.extend(ebml::void(rem).ok_or("空き領域を作れません")?);
        }
        if out.len() != len {
            return Ok(Err(len + 2));
        }
        return Ok(Ok((out, map)));
    }
    Err("SeekHead の位置が定まりません".into())
}

// ---------- 書き込み ----------

/// ファイルに書き込む変更 (位置とバイト列)
struct Patch {
    off: u64,
    bytes: Vec<u8>,
}

pub fn write(src: &Path, dst: &Path, edits: &[Edit], remove_all: bool) -> Result<bool, String> {
    let mut f = File::open(src).map_err(io)?;
    let st = parse(&mut f)?;
    if remove_all && (!st.complete || st.seg_end != st.file_len) {
        return Err("ファイル全体のメタデータを確認できないため、すべて削除できません".into());
    }
    let rel = |off: u64| off - st.seg_data;
    let header_len = st.header_end - st.seg_data;
    if header_len > MAX_HEADER {
        return Err("ヘッダー部分が大きすぎます".into());
    }

    // ヘッダー領域の要素
    let mut items: Vec<Item> = Vec::new();
    for el in st.els.iter().filter(|e| e.off < st.header_end && e.id != VOID) {
        let mut body = vec![0u8; el.size as usize];
        f.seek(SeekFrom::Start(el.off + el.hlen as u64)).map_err(io)?;
        f.read_exact(&mut body).map_err(io)?;
        items.push(Item { id: el.id, old_rel: Some(rel(el.off)), body, size_len: el.size_len });
    }
    let original_items = items.clone();
    let tail: Vec<&L1> = st.els.iter().filter(|e| e.off >= st.header_end).collect();
    let mut patches = Vec::new();

    // --- Info ---
    let info_edits = remove_all || edits.iter().any(|e| !matches!(e, Edit::Simple { .. }));
    if info_edits {
        for info in items.iter_mut().filter(|i| i.id == INFO) {
            info.body = edit_info(&info.body, edits, remove_all)?;
        }
        for el in tail.iter().filter(|e| e.id == INFO) {
            let old = read_body(&mut f, el)?;
            let body = edit_info(&old, edits, remove_all)?;
            if body != old {
                let bytes = fit(INFO, &body, el.size_len, el.total() as usize)
                    .ok_or("末尾の Info に変更を書き込む空きがありません")?;
                patches.push(Patch { off: el.off, bytes });
            }
        }
        if !st.els.iter().any(|el| el.id == INFO) && edits.iter().any(|e| matches!(e, Edit::Title(Some(_)) | Edit::DateUtc(Some(_)))) {
            return Err("Info が見つかりません".into());
        }
    }

    // --- Tags ---
    let simple: Vec<(&'static str, Option<String>)> = edits
        .iter()
        .filter_map(|e| match e {
            Edit::Simple { name, value } => Some((*name, value.clone())),
            _ => None,
        })
        .collect();
    // 末尾 (Cluster より後) の Tags: (要素, 新しい中身。None は削除)
    let mut tail_tags: Vec<(&L1, Option<Vec<u8>>)> = Vec::new();
    for el in tail.iter().filter(|e| e.id == TAGS) {
        tail_tags.push((el, Some(read_body(&mut f, el)?)));
    }
    if remove_all {
        items.retain(|i| i.id != TAGS);
        for t in tail_tags.iter_mut() {
            t.1 = None;
        }
    }
    if !simple.is_empty() {
        let mut done = Vec::new();
        for it in items.iter_mut().filter(|i| i.id == TAGS) {
            it.body = edit_tags(&it.body, &simple, &mut done)?;
        }
        for (_, body) in tail_tags.iter_mut() {
            if let Some(b) = body {
                *b = edit_tags(b, &simple, &mut done)?;
            }
        }
        let missing: Vec<u8> = simple
            .iter()
            .filter(|(n, v)| v.is_some() && !done.contains(n))
            .flat_map(|(n, v)| new_simple(n, v.as_deref().unwrap()))
            .collect();
        if !missing.is_empty() {
            let mut added = false;
            for it in items.iter_mut().filter(|i| i.id == TAGS) {
                if add_to_global(&mut it.body, &missing)? {
                    added = true;
                    break;
                }
            }
            if !added {
                for (_, body) in tail_tags.iter_mut() {
                    if let Some(b) = body {
                        if add_to_global(b, &missing)? {
                            added = true;
                            break;
                        }
                    }
                }
            }
            if !added {
                let tag = new_global_tag(&missing);
                if let Some(it) = items.iter_mut().find(|i| i.id == TAGS) {
                    // 全体向けの Tag が無い Tags に足す
                    it.body = edit_body(&it.body, |rest| {
                        let mut v = rest.to_vec();
                        v.extend_from_slice(&tag);
                        Ok(v)
                    })?;
                } else if let Some((_, Some(b))) = tail_tags.iter_mut().find(|t| t.1.is_some()) {
                    *b = edit_body(b, |rest| {
                        let mut v = rest.to_vec();
                        v.extend_from_slice(&tag);
                        Ok(v)
                    })?;
                } else {
                    // Tags が無いので新しく作る (SeekHead・Info・Tracks の後ろ)
                    let at = items
                        .iter()
                        .rposition(|i| matches!(i.id, SEEKHEAD | INFO | TRACKS))
                        .map(|i| i + 1)
                        .unwrap_or(items.len());
                    items.insert(at, Item { id: TAGS, old_rel: None, body: tag, size_len: 1 });
                }
            }
        }
    }
    // 空になった Tags 要素は取り除く
    let empty_tags = |b: &[u8]| -> Result<bool, String> {
        let (_, rest) = strip_crc(b);
        Ok(children(rest)?.iter().all(|c| c.id == VOID))
    };
    let mut kept = Vec::new();
    for it in items.into_iter() {
        if it.id == TAGS && empty_tags(&it.body)? {
            continue;
        }
        kept.push(it);
    }
    let mut items = kept;
    for t in tail_tags.iter_mut() {
        if t.1.as_deref().map(empty_tags).transpose()?.unwrap_or(false) {
            t.1 = None;
        }
    }

    // 変更が無ければ何もしない
    let header_changed = items.len() != original_items.len()
        || items.iter().zip(&original_items).any(|(a, b)| a.id != b.id || a.body != b.body);
    let mut tail_changed = Vec::new();
    for (el, body) in &tail_tags {
        let old = read_body(&mut f, el)?;
        if body.as_deref() != Some(&old[..]) {
            tail_changed.push((*el, body.clone()));
        }
    }
    if !header_changed && tail_changed.is_empty() && patches.is_empty() {
        return Ok(false);
    }

    // --- 末尾の Tags の置き場所 ---
    let can_append = st.complete && st.seg_end == st.file_len;
    let mut map: PosMap = HashMap::new();
    let mut added: Vec<(u32, u64)> = Vec::new();
    // 末尾に追加する要素。truncate_at から書く
    let mut truncate_at = st.file_len;
    let mut appended: Vec<(u32, Vec<u8>, Option<u64>)> = Vec::new(); // (ID, 要素全体, 元の相対位置)
    let following_void = |el: &L1| -> u64 {
        let mut end = el.end();
        for e in tail.iter().filter(|e| e.off >= el.end()) {
            if e.off == end && e.id == VOID {
                end = e.end();
            } else {
                break;
            }
        }
        end - el.off
    };
    let is_last = |el: &L1| tail.iter().all(|e| e.off < el.end() || e.id == VOID);
    for (el, body) in &tail_changed {
        match body {
            None => {
                patches.push(Patch { off: el.off, bytes: ebml::void(el.total() as usize).ok_or("空き領域を作れません")? });
                map.insert(rel(el.off), None);
            }
            Some(b) => {
                let space = following_void(el) as usize;
                if can_append && is_last(el) {
                    // ファイルの最後の要素なら、その位置に書き直す (長さが変わってよい)
                    truncate_at = el.off;
                    appended.push((TAGS, element(TAGS, b), Some(rel(el.off))));
                } else if let Some(bytes) = fit(TAGS, b, el.size_len, space) {
                    patches.push(Patch { off: el.off, bytes });
                } else if can_append {
                    patches.push(Patch { off: el.off, bytes: ebml::void(el.total() as usize).ok_or("空き領域を作れません")? });
                    appended.push((TAGS, element(TAGS, b), Some(rel(el.off))));
                } else {
                    return Err("タグを書き込む場所がありません (ファイルの末尾に追加できない構造です)".into());
                }
            }
        }
    }

    // 末尾に追加する要素の位置を決める
    let place_appended = |appended: &[(u32, Vec<u8>, Option<u64>)],
                          map: &mut PosMap,
                          added: &mut Vec<(u32, u64)>,
                          truncate_at: u64,
                          delta: u64| {
        let mut p = truncate_at + delta;
        for (id, bytes, old) in appended {
            let new_rel = p - st.seg_data;
            match old {
                // 移した要素は、古い項目を消して SeekHead の最後に新しく足す。
                // 元の SeekHead にその要素の項目が無くても見つけられ、ExifTool (同じ種類の項目は最後のものを使う)
                // も移した方を読む
                Some(o) if *o != new_rel => {
                    map.insert(*o, None);
                    added.push((*id, new_rel));
                }
                Some(_) => {}
                None => added.push((*id, new_rel)),
            }
            p += bytes.len() as u64;
        }
    };

    // --- ヘッダー領域 ---
    // 入りきらなければ、(1) 変更した Tags を末尾へ移す、(2) ヘッダー領域を delta バイト広げる
    let mut delta = 0u64;
    let mut header_bytes: Option<Vec<u8>> = None;
    let mut moved_tags = false;
    for _ in 0..4 {
        let mut m = map.clone();
        let mut a = added.clone();
        place_appended(&appended, &mut m, &mut a, truncate_at, delta);
        if delta > 0 {
            // Cluster 以降はすべて後ろへずれる
            for e in tail.iter().filter(|e| e.id != VOID) {
                m.entry(rel(e.off)).or_insert(Some(rel(e.off) + delta));
            }
        }
        // ヘッダー領域から消えた要素は SeekHead からも消す
        for o in original_items.iter().filter_map(|o| o.old_rel) {
            let present = items.iter().any(|i| i.old_rel == Some(o)) || appended.iter().any(|x| x.2 == Some(o));
            if !present {
                m.insert(o, None);
            }
        }
        let need_rewrite = header_changed || !m.is_empty() || !a.is_empty();
        if !need_rewrite {
            break;
        }
        let mut cand = items.clone();
        if delta > 0 {
            for it in cand.iter_mut().filter(|i| i.id == CUES) {
                it.body = shift_cues(&it.body, delta)?;
            }
        }
        let to_tail = !a.is_empty() || m.values().any(|v| matches!(v, Some(n) if *n >= header_len + delta));
        if to_tail && cand.iter().all(|i| i.id != SEEKHEAD) {
            // 末尾のタグを見つけられるよう、SeekHead を作る
            let mut entries = Vec::new();
            for it in cand.iter().filter(|i| i.old_rel.is_some() && matches!(i.id, INFO | TRACKS | CHAPTERS | ATTACHMENTS)) {
                entries.push((it.id, it.old_rel.unwrap()));
            }
            for e in tail.iter().filter(|e| e.id == CUES) {
                entries.push((CUES, rel(e.off)));
            }
            // 末尾へ移した要素は a (追加する項目) に入っている
            let mut body = Vec::new();
            for (id, pos) in entries {
                body.extend(seek_entry(id, pos));
            }
            cand.insert(0, Item { id: SEEKHEAD, old_rel: None, body, size_len: 1 });
        }
        match pack(&cand, (header_len + delta) as usize, &m, &a)? {
            Ok((bytes, full)) => {
                header_bytes = Some(bytes);
                map = full;
                added = a;
                break;
            }
            Err(_) if !moved_tags && delta == 0 && can_append => {
                moved_tags = true;
                // ヘッダー領域の Tags のうち変更したものを末尾へ移す
                let moving: Vec<usize> = items
                    .iter()
                    .enumerate()
                    .filter(|(_, it)| {
                        it.id == TAGS
                            && (it.old_rel.is_none()
                                || original_items.iter().any(|o| o.old_rel == it.old_rel && o.body != it.body))
                    })
                    .map(|(i, _)| i)
                    .collect();
                for &i in moving.iter().rev() {
                    let it = items.remove(i);
                    appended.push((TAGS, element(TAGS, &it.body), it.old_rel));
                }
            }
            Err(need) if delta == 0 && st.complete => {
                delta = (need as u64).saturating_sub(header_len) + GROW_SLACK;
            }
            Err(_) => return Err("ヘッダー部分に空きが足りないため、書き込めません".into()),
        }
    }
    if header_bytes.is_none() {
        if header_changed || delta > 0 {
            return Err("ヘッダー部分に空きが足りないため、書き込めません".into());
        }
        place_appended(&appended, &mut map, &mut added, truncate_at, delta);
    }

    // --- ずらした場合: Cues と Cluster の位置 ---
    if delta > 0 {
        for el in tail.iter().filter(|e| e.id == CUES) {
            let body = read_body(&mut f, el)?;
            let bytes = fit(CUES, &shift_cues(&body, delta)?, el.size_len, following_void(el) as usize)
                .ok_or("Cues を書き換えられません")?;
            patches.push(Patch { off: el.off, bytes });
        }
        for el in tail.iter().filter(|e| e.id == CLUSTER) {
            if let Some(p) = cluster_position_patch(&mut f, el, delta)? {
                patches.push(p);
            }
        }
    }

    // --- 末尾の SeekHead ---
    for el in tail.iter().filter(|e| e.id == SEEKHEAD) {
        let body = read_body(&mut f, el)?;
        let nb = rebuild_seekhead(&body, &map, &[])?;
        if nb != body {
            let bytes = fit(SEEKHEAD, &nb, el.size_len, following_void(el) as usize)
                .ok_or("SeekHead を書き換えられません")?;
            patches.push(Patch { off: el.off, bytes });
        }
    }

    // --- Segment のサイズ ---
    let appended_len: u64 = appended.iter().map(|a| a.1.len() as u64).sum();
    let new_end = st.seg_end.min(truncate_at) + delta + appended_len;
    if !appended.is_empty() || truncate_at != st.file_len || delta > 0 {
        if let Some((off, len)) = st.seg_size_field {
            let size = new_end - st.seg_data;
            if ebml::size_len_needed(size) > len {
                return Err("Segment のサイズを書き換えられません".into());
            }
            patches.push(Patch { off, bytes: ebml::size_bytes(size, len) });
        }
    }

    // --- 作業用ファイルに書く ---
    // patches の位置は元ファイルでの位置。Cluster 以降は delta だけずれる
    let shifted = |off: u64| if off >= st.header_end { off + delta } else { off };
    let mut out = if delta == 0 {
        drop(f);
        std::fs::copy(src, dst).map_err(|e| format!("作業用のコピーを作れませんでした: {e}"))?;
        let mut out = std::fs::OpenOptions::new().read(true).write(true).open(dst).map_err(io)?;
        if truncate_at != st.file_len {
            out.set_len(truncate_at).map_err(io)?;
        }
        if let Some(h) = &header_bytes {
            out.seek(SeekFrom::Start(st.seg_data)).map_err(io)?;
            out.write_all(h).map_err(io)?;
        }
        out.seek(SeekFrom::Start(truncate_at)).map_err(io)?;
        out
    } else {
        let mut w = std::io::BufWriter::new(File::create(dst).map_err(io)?);
        super::copy_range(&mut f, &mut w, 0, st.seg_data)?;
        w.write_all(header_bytes.as_deref().unwrap_or_default()).map_err(io)?;
        super::copy_range(&mut f, &mut w, st.header_end, truncate_at - st.header_end)?;
        w.flush().map_err(io)?;
        drop(w);
        let mut out = std::fs::OpenOptions::new().read(true).write(true).open(dst).map_err(io)?;
        out.seek(SeekFrom::End(0)).map_err(io)?;
        out
    };
    for (_, bytes, _) in &appended {
        out.write_all(bytes).map_err(io)?;
    }
    for p in &patches {
        out.seek(SeekFrom::Start(shifted(p.off))).map_err(io)?;
        out.write_all(&p.bytes).map_err(io)?;
    }
    out.sync_all().map_err(io)?;
    Ok(true)
}

/// Cues の Cluster の位置を delta だけ後ろへずらす
fn shift_cues(body: &[u8], delta: u64) -> Result<Vec<u8>, String> {
    let shift = |c: &ebml::Child| -> Vec<u8> {
        let v = uint(c.body);
        if c.id == CUE_CODEC_STATE && v == 0 {
            return c.encode();
        }
        element(c.id, &uint_bytes(v + delta, c.body.len()))
    };
    edit_body(body, |rest| {
        let mut out = Vec::new();
        for p in children(rest)? {
            if p.id != CUE_POINT {
                out.extend(p.encode());
                continue;
            }
            let mut pb = Vec::new();
            for t in children(p.body)? {
                if t.id != CUE_TRACK_POSITIONS {
                    pb.extend(t.encode());
                    continue;
                }
                let mut tb = Vec::new();
                for c in children(t.body)? {
                    if matches!(c.id, CUE_CLUSTER_POSITION | CUE_CODEC_STATE) {
                        tb.extend(shift(&c));
                    } else {
                        tb.extend(c.encode());
                    }
                }
                pb.extend(element(CUE_TRACK_POSITIONS, &tb));
            }
            out.extend(element(CUE_POINT, &pb));
        }
        Ok(out)
    })
}

/// Cluster の中の Position (その Cluster 自身の位置) を delta だけずらす。同じ長さに収まらなければエラー
fn cluster_position_patch(f: &mut File, el: &L1, delta: u64) -> Result<Option<Patch>, String> {
    let mut p = el.off + el.hlen as u64;
    let end = el.end();
    // Position は先頭付近 (Timestamp の後) にある。ブロックが出てきたら探すのをやめる
    for _ in 0..4 {
        if p >= end {
            break;
        }
        let Some(h) = read_header(f, p)? else { break };
        let Some(size) = h.size else { break };
        if h.id == CLUSTER_POSITION {
            let mut b = vec![0u8; size as usize];
            f.seek(SeekFrom::Start(p + h.hlen as u64)).map_err(io)?;
            f.read_exact(&mut b).map_err(io)?;
            let nb = uint_bytes(uint(&b) + delta, b.len());
            if nb.len() != b.len() {
                return Err("Cluster の位置を書き換えられません".into());
            }
            return Ok(Some(Patch { off: p + h.hlen as u64, bytes: nb }));
        }
        if matches!(h.id, 0xA3 | 0xA0) {
            break;
        }
        p += h.hlen as u64 + size;
    }
    Ok(None)
}

// テスト・調査用: Segment 直下の要素の一覧 (ID, 相対位置, 長さ)
#[cfg(test)]
pub fn dump(path: &Path) -> Vec<(u32, u64, u64)> {
    let mut f = File::open(path).unwrap();
    let st = parse(&mut f).unwrap();
    st.els.iter().map(|e| (e.id, e.off - st.seg_data, e.total())).collect()
}

/// テスト用: Cues の CueClusterPosition と SeekHead の位置が、すべて正しい要素を指しているか
#[cfg(test)]
pub fn positions_valid(path: &Path) -> Result<(), String> {
    let els = dump(path);
    let id_at = |pos: u64| els.iter().find(|e| e.1 == pos).map(|e| e.0);
    let mut f = File::open(path).unwrap();
    let st = parse(&mut f).unwrap();
    for el in st.els.iter() {
        let body = || read_body(&mut File::open(path).unwrap(), el);
        if el.id == CUES {
            let b = body()?;
            for p in children(strip_crc(&b).1)? {
                for t in children(p.body)?.into_iter().filter(|t| t.id == CUE_TRACK_POSITIONS) {
                    for c in children(t.body)?.into_iter().filter(|c| c.id == CUE_CLUSTER_POSITION) {
                        if id_at(uint(c.body)) != Some(CLUSTER) {
                            return Err(format!("Cue が Cluster を指していない: {}", uint(c.body)));
                        }
                    }
                }
            }
        }
        if el.id == SEEKHEAD {
            let b = body()?;
            for s in children(strip_crc(&b).1)?.into_iter().filter(|s| s.id == SEEK) {
                let kids = children(s.body)?;
                let id = kids.iter().find(|k| k.id == SEEK_ID).map(|k| uint(k.body) as u32);
                let pos = kids.iter().find(|k| k.id == SEEK_POSITION).map(|k| uint(k.body));
                if let (Some(id), Some(pos)) = (id, pos) {
                    if id_at(pos) != Some(id) {
                        return Err(format!("SeekHead の {id:x} が {pos} を指しているが、そこは {:?}", id_at(pos)));
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(parse_date("2001:01:01 00:00:00Z"), Some(0));
        assert_eq!(parse_date("2001:01:01 09:00:00+09:00"), Some(0));
        assert_eq!(parse_date("2024:05:01 10:20:30Z"), Some(736_251_630_000_000_000));
        assert_eq!(parse_date("2001-01-01 00:00:01.5"), Some(1_500_000_000));
        assert_eq!(parse_date("1999:12:31 23:59:59Z"), Some(-(366 * 86_400 + 1) * 1_000_000_000));
        assert_eq!(parse_date("abc"), None);
        assert_eq!(parse_date("1600:01:01 00:00:00"), None);
        assert_eq!(parse_date("2500:01:01 00:00:00"), None);
    }

    fn fixture(elements: &[Vec<u8>]) -> Vec<u8> {
        let mut file = element(EBML_HEADER, &element(DOCTYPE, b"matroska"));
        file.extend(element_with(SEGMENT, &elements.concat(), 4));
        file
    }

    fn change(path: &Path, group: &str, name: &str, value: Option<&str>) -> crate::write::WriteResult {
        let edit = TagChange { group: group.into(), name: name.into(), value: value.map(str::to_owned) };
        super::super::apply(path.to_str().unwrap(), &[edit], false, false).unwrap()
    }

    #[test]
    fn growing_header_preserves_next_segment() {
        let first = fixture(&[element(INFO, &element(TITLE, b"x")), element(CLUSTER, &[])]);
        let second = fixture(&[element(INFO, &element(TITLE, b"second"))]);
        let original = [first, second.clone()].concat();
        super::super::with_fixture("mkv", &original, |path| {
            assert!(!change(path, "", "all", None).ok);
            assert_eq!(std::fs::read(path).unwrap(), original);
            let result = change(path, "Info", "Title", Some("longer title"));
            assert!(result.updated, "{:?}", result.errors);
            let mut f = File::open(path).unwrap();
            let st = parse(&mut f).unwrap();
            let bytes = std::fs::read(path).unwrap();
            assert!(bytes.len() > original.len());
            assert_eq!(&bytes[st.seg_end as usize..], second);
        });
    }

    #[test]
    fn remove_all_edits_info_after_cluster() {
        let retained = element(0x4d80, b"muxer");
        let info = [element(TITLE, b"private"), element(DATE_UTC, &[0; 8]), retained.clone()].concat();
        let original = fixture(&[element(CLUSTER, &[]), element(INFO, &info)]);
        super::super::with_fixture("mkv", &original, |path| {
            let result = change(path, "", "all", None);
            assert!(result.updated, "{:?}", result.errors);
            let mut f = File::open(path).unwrap();
            let st = parse(&mut f).unwrap();
            let info = st.els.iter().find(|el| el.id == INFO).unwrap();
            assert_eq!(read_body(&mut f, info).unwrap(), retained);
        });
    }

    /// SeekHead に Tags の項目が無いファイルで Tags を末尾へ移しても、SeekHead から見つかる
    #[test]
    fn moved_tags_are_listed_in_seekhead() {
        let info = element(INFO, &element(TITLE, b"t"));
        // SeekHead は Info だけを指す (位置は SeekHead の直後)
        let seek_for = |pos: u64| element(SEEKHEAD, &seek_entry(INFO, pos));
        let pos = seek_for(0).len() as u64;
        let tags = element(TAGS, &new_global_tag(&new_simple("ARTIST", "a")));
        let original = fixture(&[seek_for(pos), info, tags, element(CLUSTER, &[0; 16])]);
        super::super::with_fixture("mkv", &original, |path| {
            let long = "x".repeat(4000);
            let result = change(path, "Matroska", "Comment", Some(&long));
            assert!(result.updated, "{:?}", result.errors);
            positions_valid(path).unwrap();
            let els = dump(path);
            let tags_pos = els.iter().find(|e| e.0 == TAGS).map(|e| e.1).unwrap();
            assert!(tags_pos > els.iter().find(|e| e.0 == CLUSTER).unwrap().1, "Tags が末尾へ移っていない");
            // SeekHead の最後の Tags の項目が移した Tags を指す
            let mut f = File::open(path).unwrap();
            let st = parse(&mut f).unwrap();
            let sh = st.els.iter().find(|e| e.id == SEEKHEAD).unwrap();
            let body = read_body(&mut f, sh).unwrap();
            let last_tags = children(&body)
                .unwrap()
                .iter()
                .filter_map(|s| {
                    let kids = children(s.body).ok()?;
                    let id = uint(kids.iter().find(|k| k.id == SEEK_ID)?.body) as u32;
                    let pos = uint(kids.iter().find(|k| k.id == SEEK_POSITION)?.body);
                    (id == TAGS).then_some(pos)
                })
                .last();
            assert_eq!(last_tags, Some(tags_pos));
        });
    }

    #[test]
    fn ambiguous_translations_and_invalid_dates_preserve_original() {
        let translated = |language: &[u8], title: &[u8]| element(SIMPLE_TAG, &[
            element(TAG_NAME, b"TITLE"), element(0x447a, language), element(TAG_STRING, title),
        ].concat());
        let tag = new_global_tag(&[translated(b"eng", b"Mountain"), translated(b"jpn", "山".as_bytes())].concat());
        let original = fixture(&[element(INFO, &[]), element(TAGS, &tag), element(CLUSTER, &[])]);
        super::super::with_fixture("mkv", &original, |path| {
            for value in [Some("new title"), None] {
                assert!(!change(path, "Matroska", "Title", value).ok);
                assert_eq!(std::fs::read(path).unwrap(), original);
            }
            assert!(!change(path, "Info", "DateTimeOriginal", Some("1600:01:01 00:00:00")).ok);
            assert_eq!(std::fs::read(path).unwrap(), original);
        });
    }

    #[test]
    fn tags_edit() {
        let tag = new_global_tag(&[new_simple("TITLE", "a"), new_simple("ARTIST", "b")].concat());
        let tags = with_crc(true, tag);
        let mut done = Vec::new();
        let out = edit_tags(&tags, &[("TITLE", Some("新".into())), ("ARTIST", None)], &mut done).unwrap();
        assert_eq!(done, vec!["TITLE", "ARTIST"]);
        let (crc, rest) = strip_crc(&out);
        assert!(crc);
        assert_eq!(crc32fast::hash(rest).to_le_bytes(), out[2..6]);
        let tag = children(rest).unwrap();
        let st: Vec<_> = children(tag[0].body).unwrap().into_iter().filter(|c| c.id == SIMPLE_TAG).collect();
        assert_eq!(st.len(), 1);
        let kids = children(st[0].body).unwrap();
        assert_eq!(kids[1].body, "新".as_bytes());
    }
}
