//! 実ファイルでの書き込みテスト。EXIF_NATIVE_DIR にテスト用の動画があるときだけ実行する。
//! 必要なファイル: plain.avi, noinfo.avi, plain.mkv, plain.webm, stream.webm, stream.mkv,
//! unknown_cluster.webm, noseek.mkv, tailtags.mkv, seek_no_tags.mkv (作り方は docs/native-writer.md の 7 章)
//! ffmpeg が PATH にあれば、映像・音声データが変わっていないことも確かめる。

use std::path::{Path, PathBuf};
use std::process::Command;

use super::apply;
use crate::exiftool::{ensure_extracted, ExifTool};
use crate::write::TagChange;

fn ch(g: &str, n: &str, v: Option<&str>) -> TagChange {
    TagChange { group: g.into(), name: n.into(), value: v.map(String::from) }
}

struct Env {
    et: ExifTool,
    work: PathBuf,
    samples: PathBuf,
    base: PathBuf,
}

impl Env {
    fn copy(&self, name: &str, as_name: &str) -> String {
        let dst = self.work.join(as_name);
        std::fs::copy(self.samples.join(name), &dst).unwrap();
        dst.to_string_lossy().into_owned()
    }

    fn tags(&mut self, path: &str) -> (serde_json::Map<String, serde_json::Value>, String) {
        let mut args = vec!["-j", "-G1", "-a", "-s", "-api", "LargeFileSupport=1"];
        if super::avi_info_is_utf8(Path::new(path)) {
            args.extend(["-api", "CharsetRIFF=UTF8"]);
        }
        args.push(path);
        let (out, err) = self.et.execute(&args).unwrap();
        let v: Vec<serde_json::Map<String, serde_json::Value>> = serde_json::from_str(&out).unwrap();
        (v.into_iter().next().unwrap(), err)
    }

    fn get(&mut self, path: &str, key: &str) -> Option<String> {
        let (m, _) = self.tags(path);
        m.get(key).map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(a) => a.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect::<Vec<_>>().join(", "),
            o => o.to_string(),
        })
    }

    /// ExifTool の警告が無いこと
    fn no_warning(&mut self, path: &str) {
        let (m, err) = self.tags(path);
        assert!(!m.contains_key("ExifTool:Warning"), "{path}: {:?}", m.get("ExifTool:Warning"));
        assert!(err.trim().is_empty(), "{path}: {err}");
    }
}

fn has_ffmpeg() -> bool {
    Command::new("ffmpeg").arg("-version").output().is_ok_and(|o| o.status.success())
}

/// パケット単位の MD5 (コピーなので、データが変わっていなければ同じ)
fn packets(path: &str) -> String {
    let o = Command::new("ffmpeg")
        .args(["-v", "error", "-i", path, "-map", "0", "-c", "copy", "-f", "framemd5", "-"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.starts_with('#')).collect::<Vec<_>>().join("\n")
}

/// 最後まで読めて、エラーが出ないこと
fn decodes(path: &str) {
    let o = Command::new("ffmpeg").args(["-v", "error", "-i", path, "-f", "null", "-"]).output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success() && err.trim().is_empty(), "{path}: {err}");
}

fn check_media(original: &str, edited: &str) {
    if !edited.ends_with(".avi") {
        super::matroska::positions_valid(Path::new(edited)).unwrap_or_else(|e| panic!("{edited}: {e}"));
    }
    if has_ffmpeg() {
        assert_eq!(packets(original), packets(edited), "{edited}: データが変わった");
        decodes(edited);
    }
}

fn size(p: &str) -> u64 {
    std::fs::metadata(p).unwrap().len()
}

#[test]
fn native_write_scenarios() {
    let Ok(dir) = std::env::var("EXIF_NATIVE_DIR") else { return };
    let base = std::env::temp_dir().join(format!("exif-native-test-{}", std::process::id()));
    let work = base.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let et = ExifTool::start(&ensure_extracted(&base.join("et")).unwrap()).unwrap();
    let mut env = Env { et, work, samples: PathBuf::from(&dir), base };
    let src = |n: &str| PathBuf::from(&dir).join(n).to_string_lossy().into_owned();

    // ---------- AVI ----------
    // A. 日本語・改行・日時。ヘッダー内に収まるのでファイルサイズは変わらない
    let avi = env.copy("plain.avi", "動画.avi");
    let r = apply(
        &avi,
        &[
            ch("RIFF", "Title", Some("日本語のタイトル")),
            ch("RIFF", "Comment", Some("1 行目\n2 行目")),
            ch("RIFF", "DateCreated", Some("2024:05:01 10:20:30")),
            ch("RIFF", "Artist", None),
            ch("RIFF", "ImageWidth", Some("1")),
        ],
        false,
        false,
    )
    .unwrap();
    assert!(r.ok && r.updated, "{:?}", r.errors);
    assert_eq!(r.warnings.len(), 1, "書けないタグの警告");
    assert_eq!(size(&avi), size(&src("plain.avi")));
    assert_eq!(env.get(&avi, "RIFF:Title").as_deref(), Some("日本語のタイトル"));
    assert_eq!(env.get(&avi, "RIFF:Comment").as_deref(), Some("1 行目\n2 行目"));
    assert_eq!(env.get(&avi, "RIFF:DateCreated").as_deref(), Some("2024:05:01 10:20:30"));
    assert_eq!(env.get(&avi, "RIFF:Artist"), None);
    env.no_warning(&avi);
    check_media(&src("plain.avi"), &avi);

    // B. 長い値: ヘッダー領域を広げる (2048 の倍数)。その後の短い値はその場で書ける
    let long = "長い説明。".repeat(1500);
    let r = apply(&avi, &[ch("RIFF", "Comment", Some(&long))], false, false).unwrap();
    assert!(r.ok && r.updated, "{:?}", r.errors);
    let grown = size(&avi);
    assert!(grown > size(&src("plain.avi")) && (grown - size(&src("plain.avi"))) % 2048 == 0);
    assert_eq!(env.get(&avi, "RIFF:Comment").as_deref(), Some(long.as_str()));
    assert_eq!(env.get(&avi, "RIFF:Title").as_deref(), Some("日本語のタイトル"));
    env.no_warning(&avi);
    check_media(&src("plain.avi"), &avi);
    let r = apply(&avi, &[ch("RIFF", "Comment", Some("短く"))], false, false).unwrap();
    assert!(r.ok && r.updated);
    assert_eq!(size(&avi), grown);
    check_media(&src("plain.avi"), &avi);

    // C. 同じ値なら書き換えない
    let r = apply(&avi, &[ch("RIFF", "Comment", Some("短く"))], false, false).unwrap();
    assert!(r.ok && !r.updated);

    // D. INFO が無い AVI に追加 → すべて削除
    let avi2 = env.copy("noinfo.avi", "noinfo.avi");
    let r = apply(&avi2, &[ch("RIFF", "Title", Some("追加"))], false, false).unwrap();
    assert!(r.ok && r.updated, "{:?}", r.errors);
    assert_eq!(env.get(&avi2, "RIFF:Title").as_deref(), Some("追加"));
    let r = apply(&avi2, &[ch("", "all", None)], false, false).unwrap();
    assert!(r.ok && r.updated);
    assert_eq!(env.get(&avi2, "RIFF:Title"), None);
    assert_eq!(env.get(&avi2, "RIFF:Software"), None);
    check_media(&src("noinfo.avi"), &avi2);

    // E. バックアップと更新日時
    let avi3 = env.copy("plain.avi", "backup.avi");
    let old_time = std::fs::metadata(&avi3).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    let r = apply(&avi3, &[ch("RIFF", "Title", Some("b"))], true, true).unwrap();
    assert!(r.ok && r.updated);
    assert_eq!(std::fs::metadata(&avi3).unwrap().modified().unwrap(), old_time);
    let backup = format!("{avi3}_original");
    assert_eq!(std::fs::read(&backup).unwrap(), std::fs::read(src("plain.avi")).unwrap());
    // 作業用ファイルが残っていない
    let leftovers: Vec<_> = std::fs::read_dir(&env.work)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());

    // ---------- MKV / WebM ----------
    // F. Info とタグ (CRC 付き)。ヘッダー内に収まる
    let mkv = env.copy("plain.mkv", "動画.mkv");
    let r = apply(
        &mkv,
        &[
            ch("Info", "Title", Some("新しいタイトル")),
            ch("Info", "DateTimeOriginal", Some("2023:12:24 18:30:00Z")),
            ch("Matroska", "Artist", Some("別の作者")),
            ch("Matroska", "Description", Some("説明\n2 行目")),
            ch("Matroska", "Encoder", None),
            ch("Track1", "Encoder", Some("x")),
        ],
        false,
        false,
    )
    .unwrap();
    assert!(r.ok && r.updated, "{:?}", r.errors);
    assert_eq!(r.warnings.len(), 1);
    assert_eq!(size(&mkv), size(&src("plain.mkv")));
    assert_eq!(env.get(&mkv, "Info:Title").as_deref(), Some("新しいタイトル"));
    assert_eq!(env.get(&mkv, "Info:DateTimeOriginal").as_deref(), Some("2023:12:24 18:30:00Z"));
    assert_eq!(env.get(&mkv, "Matroska:Artist").as_deref(), Some("別の作者"));
    assert_eq!(env.get(&mkv, "Matroska:Description").as_deref(), Some("説明\n2 行目"));
    assert_eq!(env.get(&mkv, "Matroska:Encoder"), None);
    assert!(env.get(&mkv, "Track1:Encoder").is_some(), "トラックのタグは残る");
    env.no_warning(&mkv);
    check_media(&src("plain.mkv"), &mkv);

    // G. 長い値: Tags を末尾へ移す。SeekHead から見つかる
    let long = "とても長い説明文。".repeat(3000);
    let r = apply(&mkv, &[ch("Matroska", "Description", Some(&long))], false, false).unwrap();
    assert!(r.ok && r.updated, "{:?}", r.errors);
    assert_eq!(env.get(&mkv, "Matroska:Description").as_deref(), Some(long.as_str()));
    assert_eq!(env.get(&mkv, "Matroska:Artist").as_deref(), Some("別の作者"));
    assert_eq!(env.get(&mkv, "Info:Title").as_deref(), Some("新しいタイトル"));
    env.no_warning(&mkv);
    check_media(&src("plain.mkv"), &mkv);
    // 続けて末尾の Tags を短くする (末尾なのでそのまま縮む)
    let r = apply(&mkv, &[ch("Matroska", "Description", Some("短い"))], false, false).unwrap();
    assert!(r.ok && r.updated, "{:?}", r.errors);
    assert_eq!(env.get(&mkv, "Matroska:Description").as_deref(), Some("短い"));
    env.no_warning(&mkv);
    check_media(&src("plain.mkv"), &mkv);
    // 長いタイトル (Info が大きくなる → ヘッダー領域を広げる)
    let before = size(&mkv);
    let title = "タイトル".repeat(40);
    let r = apply(&mkv, &[ch("Info", "Title", Some(&title))], false, false).unwrap();
    assert!(r.ok && r.updated, "{:?}", r.errors);
    assert_eq!(env.get(&mkv, "Info:Title").as_deref(), Some(title.as_str()));
    assert!(size(&mkv) > before, "ヘッダー領域が広がっていない");
    env.no_warning(&mkv);
    check_media(&src("plain.mkv"), &mkv);
    // 広げた余白があるので、次の書き込みではサイズが変わらない
    let grown = size(&mkv);
    let r = apply(&mkv, &[ch("Info", "Title", Some("短い題"))], false, false).unwrap();
    assert!(r.ok && r.updated, "{:?}", r.errors);
    assert_eq!(size(&mkv), grown);
    check_media(&src("plain.mkv"), &mkv);

    // H. すべて削除
    let r = apply(&mkv, &[ch("", "all", None)], false, false).unwrap();
    assert!(r.ok && r.updated, "{:?}", r.errors);
    let (m, _) = env.tags(&mkv);
    assert!(!m.contains_key("Info:Title") && !m.contains_key("Info:DateTimeOriginal"));
    assert!(!m.keys().any(|k| k.starts_with("Matroska:") && !k.contains("EBML") && !k.contains("DocType")), "{m:?}");
    env.no_warning(&mkv);
    check_media(&src("plain.mkv"), &mkv);
    // 削除後に新しくタグを作る
    let r = apply(&mkv, &[ch("Matroska", "Artist", Some("再追加"))], false, false).unwrap();
    assert!(r.ok && r.updated, "{:?}", r.errors);
    assert_eq!(env.get(&mkv, "Matroska:Artist").as_deref(), Some("再追加"));
    env.no_warning(&mkv);
    check_media(&src("plain.mkv"), &mkv);

    // I. WebM、サイズ不明の Segment / Cluster、SeekHead なし、Tags が末尾
    for name in ["plain.webm", "stream.webm", "stream.mkv", "unknown_cluster.webm", "noseek.mkv", "tailtags.mkv", "seek_no_tags.mkv"] {
        let p = env.copy(name, &format!("t_{name}"));
        let r = apply(&p, &[ch("Matroska", "Artist", Some("作者 A")), ch("Info", "Title", Some("題"))], false, false).unwrap();
        assert!(r.ok && r.updated, "{name}: {:?}", r.errors);
        assert_eq!(env.get(&p, "Matroska:Artist").as_deref(), Some("作者 A"), "{name}");
        assert_eq!(env.get(&p, "Info:Title").as_deref(), Some("題"), "{name}");
        env.no_warning(&p);
        check_media(&src(name), &p);
        let long = format!("{name} の長いコメント。").repeat(2000);
        let r = apply(&p, &[ch("Matroska", "Comment", Some(&long))], false, false).unwrap();
        assert!(r.ok && r.updated, "{name}: {:?}", r.errors);
        assert_eq!(env.get(&p, "Matroska:Comment").as_deref(), Some(long.as_str()), "{name}");
        assert_eq!(env.get(&p, "Matroska:Artist").as_deref(), Some("作者 A"), "{name}");
        env.no_warning(&p);
        check_media(&src(name), &p);
        let r = apply(&p, &[ch("Matroska", "Comment", None), ch("Matroska", "Keywords", Some("海, 夏"))], false, false).unwrap();
        assert!(r.ok && r.updated, "{name}: {:?}", r.errors);
        assert_eq!(env.get(&p, "Matroska:Comment"), None, "{name}");
        assert_eq!(env.get(&p, "Matroska:Keywords").as_deref(), Some("海, 夏"), "{name}");
        env.no_warning(&p);
        check_media(&src(name), &p);
        // ヘッダー領域を広げる (長いタイトル)
        let title = format!("{name} のタイトル").repeat(60);
        let r = apply(&p, &[ch("Info", "Title", Some(&title))], false, false).unwrap();
        assert!(r.ok && r.updated, "{name}: {:?}", r.errors);
        assert_eq!(env.get(&p, "Info:Title").as_deref(), Some(title.as_str()), "{name}");
        assert_eq!(env.get(&p, "Matroska:Keywords").as_deref(), Some("海, 夏"), "{name}");
        env.no_warning(&p);
        check_media(&src(name), &p);
        println!("{name}: {} -> {} bytes", size(&src(name)), size(&p));
    }

    drop(env.et);
    let _ = std::fs::remove_dir_all(&env.base);
}

/// 1GB を超える AVI (OpenDML の索引と AVIX を持つ) で、ヘッダー領域を広げたときに索引の位置が直るか。
/// EXIF_NATIVE_BIG にファイルを指定したときだけ実行 (作り方は docs/native-writer.md の 7 章)
#[test]
fn native_write_odml_avi() {
    let Ok(big) = std::env::var("EXIF_NATIVE_BIG") else { return };
    let base = std::env::temp_dir().join(format!("exif-native-big-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let mut et = ExifTool::start(&ensure_extracted(&base.join("et")).unwrap()).unwrap();
    let p = base.join("odml.avi").to_string_lossy().into_owned();
    std::fs::copy(&big, &p).unwrap();

    let long = "長い説明。".repeat(2000);
    let r = apply(&p, &[ch("RIFF", "Comment", Some(&long)), ch("RIFF", "Title", Some("大きい AVI"))], false, false).unwrap();
    assert!(r.ok && r.updated, "{:?}", r.errors);
    assert!(size(&p) > size(&big), "ヘッダー領域が広がっていない");
    check_odml_index(&p);
    let (out, _) = et.execute(&["-j", "-api", "CharsetRIFF=UTF8", "-api", "LargeFileSupport=1", "-RIFF:Title", "-RIFF:Comment", &p]).unwrap();
    assert!(out.contains("大きい AVI") && out.contains(&long[..30]), "{}", &out[..out.len().min(300)]);
    if has_ffmpeg() {
        assert_eq!(packets(&big), packets(&p), "データが変わった");
        decodes(&p);
        // OpenDML の索引を使って途中へシークできる
        let o = Command::new("ffmpeg").args(["-v", "error", "-ss", "90", "-i", &p, "-frames:v", "1", "-f", "null", "-"]).output().unwrap();
        assert!(o.status.success() && o.stderr.is_empty(), "{}", String::from_utf8_lossy(&o.stderr));
    }
    drop(et);
    let _ = std::fs::remove_dir_all(&base);
}

/// OpenDML のスーパー索引が ix## を指し、ix## の各エントリが映像・音声のチャンクを指しているか
fn check_odml_index(path: &str) {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).unwrap();
    let mut head = vec![0u8; 256 * 1024];
    let n = f.read(&mut head).unwrap();
    let head = &head[..n];
    let at = |f: &mut std::fs::File, pos: u64, len: usize| {
        let mut b = vec![0u8; len];
        f.seek(SeekFrom::Start(pos)).unwrap();
        f.read_exact(&mut b).unwrap();
        b
    };
    let le32 = |b: &[u8]| u32::from_le_bytes(b[..4].try_into().unwrap());
    let le64 = |b: &[u8]| u64::from_le_bytes(b[..8].try_into().unwrap());
    let mut checked = 0;
    let mut i = 0;
    while let Some(k) = head[i..].windows(4).position(|w| w == b"indx") {
        let d = i + k + 8;
        i = d;
        assert_eq!(head[d + 3], 0, "スーパー索引ではない");
        let entries = le32(&head[d + 4..]) as usize;
        for e in 0..entries {
            let off = le64(&head[d + 24 + e * 16..]);
            let ix = at(&mut f, off, 32);
            assert_eq!(&ix[0..2], b"ix", "indx が ix## を指していない: {off}");
            let base = le64(&ix[20..]);
            let n = le32(&ix[12..]) as usize;
            // 最初と最後のエントリが指すチャンクの ID を確かめる
            for j in [0, n - 1] {
                let ent = at(&mut f, off + 32 + j as u64 * 8, 8);
                let chunk = base + le32(&ent) as u64 - 8;
                let id = at(&mut f, chunk, 4);
                assert!(id[2..4] == *b"dc" || id[2..4] == *b"wb", "ix のエントリがチャンクを指していない: {:?}", id);
            }
            checked += 1;
        }
    }
    assert!(checked >= 2, "OpenDML の索引が見つからない");
}
