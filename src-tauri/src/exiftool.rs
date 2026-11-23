//! 埋め込んだ ExifTool の展開と、`-stay_open` で常駐させたプロセスとのやり取り。

use std::fs;
use std::io::{BufRead, BufReader, Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use sha2::{Digest, Sha256};

/// ビルド時に exe に埋め込む ExifTool 一式 (公式配布の Windows 64bit zip そのまま)。
/// `node scripts/fetch-exiftool.mjs` で取得する。
static EXIFTOOL_ZIP: &[u8] = include_bytes!("../vendor/exiftool.zip");
const EXIFTOOL_VERSION: &str = include_str!("../vendor/exiftool.version");

/// stderr の 1 コマンド分を待つ上限。大きな動画でもこれを超えることはまずない。
const STDERR_TIMEOUT: Duration = Duration::from_secs(120);
/// 読み取りなど通常のコマンドの応答を待つ上限。超えたら ExifTool が固まったとみなして止め、次の操作で起動し直す
pub const READ_TIMEOUT: Duration = Duration::from_secs(300);
/// 書き込みの上限 (大きな動画はファイル全体を書き直すので長めにする)
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(3600);

/// 埋め込み zip を `base_dir/<version>-<hash>/` に展開し、exiftool.exe のパスを返す。
/// 展開済み (完了マーカーあり) なら何もしない。
pub fn ensure_extracted(base_dir: &Path) -> Result<PathBuf, String> {
    let hash = Sha256::digest(EXIFTOOL_ZIP);
    let short: String = hash.iter().take(6).map(|b| format!("{b:02x}")).collect();
    let dir = base_dir.join(format!("{}-{}", EXIFTOOL_VERSION.trim(), short));
    let exe = dir.join("exiftool.exe");
    let marker = dir.join(".complete");
    if exe.is_file() && marker.is_file() {
        return Ok(exe);
    }

    // 途中で落ちても壊れた状態を残さないよう、一時フォルダに展開してから名前を変える
    let tmp = base_dir.join(format!(".tmp-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| format!("展開先を作れません: {e}"))?;

    let mut archive = zip::ZipArchive::new(Cursor::new(EXIFTOOL_ZIP))
        .map_err(|e| format!("ExifTool の zip が壊れています: {e}"))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(name) = entry.enclosed_name() else { continue };
        // 先頭の "exiftool-13.xx_64/" を取り除く
        let rel: PathBuf = name.components().skip(1).collect();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let rel = if rel == Path::new("exiftool(-k).exe") {
            PathBuf::from("exiftool.exe")
        } else {
            rel
        };
        let out = tmp.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(p) = out.parent() {
                fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut f = fs::File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
        }
    }
    fs::write(tmp.join(".complete"), b"").map_err(|e| e.to_string())?;

    let _ = fs::remove_dir_all(&dir);
    fs::rename(&tmp, &dir).map_err(|e| format!("ExifTool を配置できません: {e}"))?;
    Ok(exe)
}

/// 引数ファイルは 1 行 = 1 引数。ExifTool は通常の行について
/// 「行頭の空白」と「`-タグ=` の `=` の直後の空白 1 つ」を削る。
/// そこでタグ代入の引数は `=` の直後に空白を 1 つ足しておき、値の前後の空白を保つ。
/// 値に改行を含めたいときは、呼び出し側で `-E` と HTML 文字参照 (`&#xa;`) を使う
/// (`#[CSTR]` 形式は `$` `@` の前に `\` が付いてしまうため使わない)。
pub(crate) fn encode_arg(arg: &str) -> Result<String, String> {
    if arg.contains('\n') || arg.contains('\r') {
        return Err("引数に改行を含めることはできません (-E と &#xa; を使ってください)".into());
    }
    if is_tag_assignment(arg) {
        let i = arg.find('=').unwrap();
        return Ok(format!("{} {}", &arg[..=i], &arg[i + 1..]));
    }
    Ok(arg.to_string())
}

/// ExifTool が `=` 直後の空白を削る対象の形 (`-Group:Tag=` / `-Tag+=` / `-Tag<=` など) か
fn is_tag_assignment(arg: &str) -> bool {
    let Some(rest) = arg.strip_prefix('-') else { return false };
    let Some(eq) = rest.find('=') else { return false };
    let mut name = &rest[..eq];
    name = name.strip_suffix(['+', '-', '<']).unwrap_or(name);
    name = name.strip_suffix('#').unwrap_or(name);
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':'))
}

/// `-E` を付けた書き込みで使う値のエスケープ (HTML 文字参照)
pub fn escape_value(v: &str) -> String {
    v.replace('&', "&amp;").replace('\n', "&#xa;").replace('\r', "&#xd;")
}

pub struct ExifTool {
    child: Child,
    stdin: ChildStdin,
    stdout_rx: Receiver<String>,
    stderr_rx: Receiver<String>,
    seq: u64,
}

impl ExifTool {
    pub fn start(exe: &Path) -> Result<Self, String> {
        let mut cmd = Command::new(exe);
        cmd.args([
            "-stay_open", "True", "-@", "-",
            // 全コマンド共通: 引数ファイル中のパスを UTF-8 として扱う (日本語パス対応)
            "-common_args", "-charset", "filename=utf8",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd.spawn().map_err(|e| format!("ExifTool を起動できません: {e}"))?;
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        // stdout / stderr は別スレッドで 1 行ずつ読む。待つ側で時間の上限を設けられるようにし、
        // パイプが詰まって固まるのも防ぐ
        Ok(Self { child, stdin, stdout_rx: read_lines(stdout), stderr_rx: read_lines(stderr), seq: 0 })
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// 1 コマンドを実行し (stdout, stderr) を返す。READ_TIMEOUT を超えたら ExifTool を止めてエラー
    pub fn execute(&mut self, args: &[&str]) -> Result<(String, String), String> {
        self.execute_timeout(args, READ_TIMEOUT)
    }

    pub fn execute_timeout(&mut self, args: &[&str], timeout: Duration) -> Result<(String, String), String> {
        self.seq += 1;
        let n = self.seq;
        let err_marker = format!("{{ready-err{n}}}");

        let mut input = String::new();
        for a in args {
            input.push_str(&encode_arg(a)?);
            input.push('\n');
        }
        // -echo4 は処理完了後に stderr へ出力されるので、stderr の区切りとして使う
        input.push_str(&format!("-echo4\n{err_marker}\n-execute{n}\n"));
        self.stdin
            .write_all(input.as_bytes())
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("ExifTool への送信に失敗: {e}"))?;

        let ready = format!("{{ready{n}}}");
        let deadline = std::time::Instant::now() + timeout;
        let mut out = String::new();
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match self.stdout_rx.recv_timeout(left) {
                Ok(line) if line.trim_end() == ready => break,
                Ok(line) => {
                    out.push_str(&line);
                    out.push('\n');
                }
                Err(RecvTimeoutError::Timeout) => {
                    // 固まった ExifTool は止める (with_exiftool が次の操作で起動し直す)
                    let _ = self.child.kill();
                    return Err(format!("ExifTool が {} 秒以内に応答しなかったため中断しました", timeout.as_secs()));
                }
                Err(RecvTimeoutError::Disconnected) => return Err("ExifTool が終了しました".into()),
            }
        }

        let mut err = String::new();
        loop {
            match self.stderr_rx.recv_timeout(STDERR_TIMEOUT) {
                Ok(line) if line.trim_end() == err_marker => break,
                Ok(line) => {
                    err.push_str(&line);
                    err.push('\n');
                }
                Err(_) => return Err("ExifTool のエラー出力を待てませんでした".into()),
            }
        }
        Ok((out, err))
    }

    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn stop(&mut self) {
        let _ = self.stdin.write_all(b"-stay_open\nFalse\n");
        let _ = self.stdin.flush();
        for _ in 0..20 {
            if !self.is_alive() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
    }
}

/// パイプを別スレッドで 1 行ずつ読み、チャネルへ送る (UTF-8 として読めないバイトは置き換える)
fn read_lines(pipe: impl std::io::Read + Send + 'static) -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf);
                    let line = line.trim_end_matches(['\n', '\r']).to_string();
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            }
        }
    });
    rx
}

/// プロセスを ID で強制終了する (終了時に ExifTool が処理中でロックを取れない場合に使う)
#[cfg(windows)]
pub fn kill_pid(pid: u32) {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn TerminateProcess(handle: isize, code: u32) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }
    const PROCESS_TERMINATE: u32 = 0x0001;
    // SAFETY: 取得したハンドルだけを使い、使い終わったら閉じる
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if h != 0 {
            TerminateProcess(h, 1);
            CloseHandle(h);
        }
    }
}

impl Drop for ExifTool {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 応答が上限時間を超えたら ExifTool を止めてエラーを返し、固まり続けない
    #[test]
    fn timeout_stops_exiftool() {
        let base = std::env::temp_dir().join(format!("exif-app-test-to-{}", std::process::id()));
        let mut et = ExifTool::start(&ensure_extracted(&base).unwrap()).unwrap();
        let err = et.execute_timeout(&["-ver"], Duration::ZERO).unwrap_err();
        assert!(err.contains("応答しなかった"), "{err}");
        std::thread::sleep(Duration::from_millis(300));
        assert!(!et.is_alive());
        // 新しく起動すれば使える
        let mut et = ExifTool::start(&ensure_extracted(&base).unwrap()).unwrap();
        assert!(et.execute(&["-ver"]).is_ok());
        drop(et);
        let _ = fs::remove_dir_all(&base);
    }

    /// 展開 → 起動 → 複数コマンド → 日本語パスの読み取り、を通しで確認する。
    /// EXIF_SAMPLE に日本語パスの画像を指定すると、その読み取りも確認する。
    #[test]
    fn extract_and_run() {
        let base = std::env::temp_dir().join(format!("exif-app-test-{}", std::process::id()));
        let exe = ensure_extracted(&base).expect("extract");
        // 2 回目は展開済みを使う
        assert_eq!(ensure_extracted(&base).unwrap(), exe);

        let mut et = ExifTool::start(&exe).expect("start");
        let (ver, _) = et.execute(&["-ver"]).unwrap();
        assert_eq!(ver.trim(), EXIFTOOL_VERSION.trim());

        let (_, err) = et.execute(&["-j", "C:/no/such/file.jpg"]).unwrap();
        assert!(err.contains("File not found"), "stderr: {err}");

        if let Ok(sample) = std::env::var("EXIF_SAMPLE") {
            let (out, err) = et.execute(&["-j", "-G1", "-s", &sample]).unwrap();
            assert!(err.trim().is_empty(), "stderr: {err}");
            println!("{out}");
            assert!(out.contains("\"System:FileName\""), "stdout: {out}");
        }
        drop(et);
        let _ = fs::remove_dir_all(&base);
    }

    /// 改行・バックスラッシュ・引用符・$ や @ を含む値が、そのまま書き込まれること
    /// (EXIF_WRITE_SAMPLE に書き込んでよい JPEG のコピーを指定したときだけ実行)
    #[test]
    fn write_special_characters() {
        let Ok(sample) = std::env::var("EXIF_WRITE_SAMPLE") else { return };
        let base = std::env::temp_dir().join(format!("exif-app-test-w-{}", std::process::id()));
        let exe = ensure_extracted(&base).unwrap();
        let mut et = ExifTool::start(&exe).unwrap();

        let cases = [
            "1行目\n2行目",
            "back\\slash \\n literal",
            "quote \" and 'single'",
            "$HOME @ARGV %ENV ${x}",
            "crlf\r\nline",
            "trailing backslash\\",
            "  前後に空白  ",
            "&amp; は文字どおり & <b>",
            "=始まり",
            "\tタブ\t",
        ];
        for v in cases {
            let arg = format!("-XMP-dc:Description={}", escape_value(v));
            let (_, err) = et.execute(&["-overwrite_original", "-E", &arg, &sample]).unwrap();
            assert!(!err.contains("Error"), "{v:?}: {err}");
            let (out, _) = et.execute(&["-j", "-XMP-dc:Description", &sample]).unwrap();
            let json: Vec<serde_json::Value> = serde_json::from_str(&out).unwrap();
            let got = json[0]["Description"].as_str().unwrap_or_default();
            assert_eq!(got, v, "書き込んだ値と読み出した値が違う");
        }
        drop(et);
        let _ = fs::remove_dir_all(&base);
    }
}
