// ExifTool (Windows 64bit) を取得して src-tauri/vendor/exiftool.zip に置く。
// SHA-256 を exiftool.org の公式チェックサムと照合し、一致しなければ失敗する。
// 使い方: node scripts/fetch-exiftool.mjs [バージョン]  (省略時は最新版)
import { createHash } from "node:crypto";
import { mkdir, writeFile, readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const vendorDir = join(root, "src-tauri", "vendor");
const zipPath = join(vendorDir, "exiftool.zip");
const versionPath = join(vendorDir, "exiftool.version");

async function text(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`);
  return (await res.text()).trim();
}

const version = process.argv[2] ?? (await text("https://exiftool.org/ver.txt"));
const fileName = `exiftool-${version}_64.zip`;

if (existsSync(zipPath) && existsSync(versionPath)) {
  if ((await readFile(versionPath, "utf8")).trim() === version) {
    console.log(`ExifTool ${version} は取得済みです`);
    process.exit(0);
  }
}

const checksums = await text(`https://exiftool.org/checksums-${version}.txt`);
const line = checksums.split("\n").find((l) => l.startsWith(`SHA2-256(${fileName})`));
if (!line) throw new Error(`checksums-${version}.txt に ${fileName} の SHA-256 がありません`);
const expected = line.split("=")[1].trim().toLowerCase();

console.log(`ExifTool ${version} をダウンロード中...`);
const res = await fetch(`https://sourceforge.net/projects/exiftool/files/${fileName}/download`);
if (!res.ok) throw new Error(`download: HTTP ${res.status}`);
const buf = Buffer.from(await res.arrayBuffer());

const actual = createHash("sha256").update(buf).digest("hex");
if (actual !== expected) {
  throw new Error(`SHA-256 不一致\n  期待値: ${expected}\n  実際  : ${actual}`);
}

await mkdir(vendorDir, { recursive: true });
await writeFile(zipPath, buf);
await writeFile(versionPath, version + "\n");
console.log(`OK: ${zipPath} (${(buf.length / 1024 / 1024).toFixed(1)} MB, sha256 一致)`);
