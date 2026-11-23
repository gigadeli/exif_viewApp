// exe に含まれるサードパーティのライセンスを集めて src/generated/licenses.json に書き出す。
// 「このアプリについて」画面で表示する。
// - npm: 実行時の依存 (devDependencies を除く)
// - Rust: Windows 向けビルドで通常の依存として入るクレート (ビルド時だけの依存は除く)
// - ExifTool (同梱) と OpenStreetMap (地図データ)
// 使い方: node scripts/collect-licenses.mjs
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const LICENSE_FILE = /^(licen[cs]e|copying|notice|unlicense)([-_.].*)?$/i;

const texts = new Map(); // hash -> text
const addText = (text) => {
  const t = text.replace(/\r\n/g, "\n").trim();
  const id = createHash("sha256").update(t).digest("hex").slice(0, 12);
  texts.set(id, t);
  return id;
};
const licenseFilesIn = (dir) =>
  existsSync(dir)
    ? readdirSync(dir, { withFileTypes: true })
        .filter((e) => e.isFile() && LICENSE_FILE.test(e.name))
        .map((e) => e.name)
        .sort()
    : [];

const components = [];

// ---- npm ----
const run = (cmd, args, cwd) =>
  execFileSync(cmd, args, { cwd, encoding: "utf8", maxBuffer: 64 * 1024 * 1024, shell: process.platform === "win32" });

const npmTree = JSON.parse(run("npm", ["ls", "--omit=dev", "--all", "--json"], root));
const npmSeen = new Set();
(function walk(node) {
  for (const [name, info] of Object.entries(node.dependencies ?? {})) {
    const key = `${name}@${info.version}`;
    if (!npmSeen.has(key)) {
      npmSeen.add(key);
      const dir = join(root, "node_modules", ...name.split("/"));
      const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
      const files = licenseFilesIn(dir);
      components.push({
        source: "npm",
        name,
        version: info.version,
        license: typeof pkg.license === "string" ? pkg.license : (pkg.license?.type ?? "不明"),
        textIds: files.map((f) => addText(readFileSync(join(dir, f), "utf8"))),
      });
    }
    walk(info);
  }
})(npmTree);

// ---- Rust ----
const meta = JSON.parse(
  run("cargo", ["metadata", "--format-version", "1", "--filter-platform", "x86_64-pc-windows-msvc"], join(root, "src-tauri")),
);
const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
const pkgs = new Map(meta.packages.map((p) => [p.id, p]));
const shipped = new Set();
(function walk(id) {
  for (const dep of nodes.get(id).deps) {
    // ビルドスクリプトや開発時だけの依存は exe に入らない
    if (!dep.dep_kinds.some((k) => k.kind === null)) continue;
    if (shipped.has(dep.pkg)) continue;
    shipped.add(dep.pkg);
    walk(dep.pkg);
  }
})(meta.resolve.root);

for (const id of [...shipped].sort()) {
  const p = pkgs.get(id);
  if (p.source === null) continue; // このアプリ自身
  const dir = dirname(p.manifest_path);
  const files = licenseFilesIn(dir);
  components.push({
    source: "cargo",
    name: p.name,
    version: p.version,
    license: p.license ?? (p.license_file ? "ライセンスファイル参照" : "不明"),
    textIds: files.map((f) => addText(readFileSync(join(dir, f), "utf8"))),
  });
}

// ---- 同梱物・データ ----
const exiftoolVersion = readFileSync(join(root, "src-tauri", "vendor", "exiftool.version"), "utf8").trim();
components.unshift({
  source: "bundled",
  name: "ExifTool (Phil Harvey)",
  version: exiftoolVersion,
  license: "Perl Artistic License / GPL",
  url: "https://exiftool.org/",
  textIds: [
    addText(
      `ExifTool by Phil Harvey (https://exiftool.org/)

This is free software; you can redistribute it and/or modify it under the same terms as Perl itself
(the Perl Artistic License or the GNU General Public License).

The Windows build of ExifTool bundled with this application includes a Perl interpreter (Strawberry Perl).
Its licenses are included as exiftool_files/Licenses_Strawberry_Perl.zip in the folder where
this application extracts ExifTool (%LOCALAPPDATA%\\com.local.exifapp\\exiftool\\).`,
    ),
  ],
});
components.unshift({
  source: "data",
  name: "OpenStreetMap (地図データ・タイル)",
  version: "",
  license: "ODbL (データ) / CC BY-SA 2.0 (タイル)",
  url: "https://www.openstreetmap.org/copyright",
  textIds: [
    addText(
      `Map data © OpenStreetMap contributors, available under the Open Database License (ODbL).
Map tiles are provided by the OpenStreetMap Foundation under its Tile Usage Policy.
https://www.openstreetmap.org/copyright`,
    ),
  ],
});

// ---- ライセンスファイルを同梱していないクレートの補完 ----
// MPL など著作者名を含まない本文は、同じライセンスの他のクレートの本文を使う。
// MIT / BSD-3-Clause は SPDX の標準文に Cargo.toml の著作者 (無ければリポジトリ) を入れる。
const MIT = (holder) => `MIT License

Copyright (c) ${holder}

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.`;
const BSD3 = (holder) => `BSD 3-Clause License

Copyright (c) ${holder}

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its
   contributors may be used to endorse or promote products derived from
   this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.`;

for (const c of components.filter((c) => c.textIds.length === 0)) {
  const p = [...pkgs.values()].find((p) => p.name === c.name && p.version === c.version);
  const holder = p?.authors?.length
    ? p.authors.map((a) => a.replace(/\s*<.*>/, "")).join(", ")
    : `the ${c.name} authors (${p?.repository ?? "repository unknown"})`;
  const note = `(${c.name} ${c.version} のパッケージにはライセンスファイルが含まれていないため、SPDX の標準文を表示しています)\n\n`;
  if (c.license === "MIT") c.textIds = [addText(note + MIT(holder))];
  else if (c.license === "BSD-3-Clause") c.textIds = [addText(note + BSD3(holder))];
  else {
    const same = components.find((o) => o !== c && o.license === c.license && o.textIds.length > 0);
    if (same) c.textIds = [...same.textIds];
  }
}

const missing = components.filter((c) => c.textIds.length === 0).map((c) => `${c.name}@${c.version}`);
const out = join(root, "src", "generated");
mkdirSync(out, { recursive: true });
writeFileSync(
  join(out, "licenses.json"),
  JSON.stringify({ components, texts: Object.fromEntries(texts) }),
);
console.log(
  `licenses: ${components.length} 件 (npm ${components.filter((c) => c.source === "npm").length}, ` +
    `Rust ${components.filter((c) => c.source === "cargo").length}), 本文 ${texts.size} 種類` +
    (missing.length ? `\nライセンスファイルが無いもの (ライセンス名のみ表示): ${missing.join(", ")}` : ""),
);
