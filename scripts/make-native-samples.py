"""AVI / MKV / WebM の書き込みテスト (src-tauri/src/native/file_tests.rs) 用の動画を作る。

使い方: python scripts/make-native-samples.py <出力フォルダ>
ffmpeg が PATH に必要。作ったフォルダを環境変数 EXIF_NATIVE_DIR に指定して cargo test を実行する。
"""
import os
import subprocess
import sys

out = sys.argv[1] if len(sys.argv) > 1 else "native-samples"
os.makedirs(out, exist_ok=True)
os.chdir(out)

FF = ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
      "-f", "lavfi", "-i", "testsrc=size=320x240:rate=25",
      "-f", "lavfi", "-i", "sine=frequency=440", "-t", "3"]


def ff(args, dest):
    if dest.startswith("|"):
        with open(dest[1:], "wb") as f:
            subprocess.run(FF + args + ["-"], check=True, stdout=f)
    else:
        subprocess.run(FF + args + [dest], check=True)


ff(["-c:v", "libx264", "-c:a", "aac", "-metadata", "title=元タイトル", "-metadata", "artist=作者",
    "-metadata", "creation_time=2024-05-01T10:20:30Z"], "plain.mkv")
ff(["-c:v", "libvpx", "-b:v", "300k", "-c:a", "libopus", "-metadata", "title=webm title"], "plain.webm")
ff(["-c:v", "mpeg4", "-c:a", "pcm_s16le", "-metadata", "title=AVI title", "-metadata", "artist=AVI artist"], "plain.avi")
ff(["-c:v", "mpeg4", "-c:a", "pcm_s16le", "-fflags", "+bitexact"], "noinfo.avi")
# パイプへの出力 (Segment のサイズが不明、Cues なし)
ff(["-c:v", "libvpx", "-b:v", "300k", "-c:a", "libopus", "-f", "webm"], "|stream.webm")
ff(["-c:v", "libx264", "-c:a", "aac", "-f", "matroska"], "|stream.mkv")

# 以下は ffmpeg の出力を加工して、珍しい構造のファイルを作る
def vint(b, p, keep=False):
    f = b[p]; l = 1
    while l <= 8 and not (f & (0x80 >> (l - 1))): l += 1
    v = f if keep else f & ((0x80 >> (l - 1)) - 1)
    for i in range(1, l): v = (v << 8) | b[p + i]
    return v, l

def size_bytes(n, l):
    return ((1 << (7 * l)) | n).to_bytes(l, 'big')

def elements(b):
    p = 0
    i, l = vint(b, p, True); s, sl = vint(b, p + l); p += l + sl + s
    segp = p
    i, l = vint(b, p, True); s, sl = vint(b, p + l); seg = p + l + sl; segsl = sl
    out = []; p = seg
    while p < len(b):
        i, l = vint(b, p, True); s, sl = vint(b, p + l)
        out.append(dict(id=i, off=p, hl=l + sl, sl=sl, size=s, end=p + l + sl + s)); p += l + sl + s
    return segp, seg, segsl, out

def void(n):
    for l in range(1, 9):
        d = n - 1 - l
        if d < (1 << (7 * l)) - 1: return bytes([0xEC]) + size_bytes(d, l) + bytes(d)

# 1. unknown-size clusters
b = bytearray(open('stream.webm', 'rb').read())
segp, seg, ssl, els = elements(b)
n = 0
for e in els:
    if e['id'] == 0x1F43B675:
        L = e['sl']
        b[e['off'] + 4:e['off'] + 4 + L] = (((1 << (8 * L)) - 1) >> (L - 1)).to_bytes(L, 'big'); n += 1
open('unknown_cluster.webm', 'wb').write(b); print('unknown clusters', n)

# 2. no SeekHead
b = bytearray(open('plain.mkv', 'rb').read())
segp, seg, ssl, els = elements(b)
sh = [e for e in els if e['id'] == 0x114D9B74][0]
b[sh['off']:sh['end']] = void(sh['end'] - sh['off'])
open('noseek.mkv', 'wb').write(b)

# 3. Tags at the end of the file
b = bytearray(open('plain.mkv', 'rb').read())
segp, seg, ssl, els = elements(b)
t = [e for e in els if e['id'] == 0x1254C367][0]
tags = bytes(b[t['off']:t['end']])
b[t['off']:t['end']] = void(t['end'] - t['off'])
newrel = len(b) - seg
b += tags
sh = [e for e in els if e['id'] == 0x114D9B74][0]
p = sh['off'] + sh['hl']
while p < sh['end']:
    i, l = vint(b, p, True); s, sl = vint(b, p + l); body = p + l + sl
    if i == 0x4DBB:
        q = body; sid = None
        while q < body + s:
            ci, cl = vint(b, q, True); cs, csl = vint(b, q + cl); cb = q + cl + csl
            if ci == 0x53AB: sid = bytes(b[cb:cb + cs])
            if ci == 0x53AC and sid == bytes.fromhex('1254C367'):
                b[cb:cb + cs] = newrel.to_bytes(cs, 'big'); print('seek tags ->', newrel, cs)
            q = cb + cs
    p = body + s
p = sh['off'] + sh['hl']
if b[p] == 0xBF:
    b[p:p + 6] = void(6); print('seekhead crc removed')
b[seg - ssl:seg] = size_bytes(len(b) - seg, ssl)
open('tailtags.mkv', 'wb').write(b)

# 4. SeekHead はあるが Tags の項目が無い (Tags を末尾へ移したとき SeekHead に足す必要がある)
b = bytearray(open('plain.mkv', 'rb').read())
segp, seg, ssl, els = elements(b)
sh = [e for e in els if e['id'] == 0x114D9B74][0]
q = sh['off'] + sh['hl']
while q < sh['end']:
    ci, cl = vint(b, q, True); cs, csl = vint(b, q + cl); cb = q + cl + csl
    if ci == 0xBF:  # 中身を変えるので CRC は外す
        b[q:cb + cs] = void(cb + cs - q)
    if ci == 0x4DBB:
        r = cb; sid = None
        while r < cb + cs:
            ri, rl = vint(b, r, True); rs, rsl = vint(b, r + rl); rb = r + rl + rsl
            if ri == 0x53AB: sid = bytes(b[rb:rb + rs])
            r = rb + rs
        if sid == bytes.fromhex('1254C367'):
            b[q:cb + cs] = void(cb + cs - q)
    q = cb + cs
open('seek_no_tags.mkv', 'wb').write(b)
