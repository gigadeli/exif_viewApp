# EXIF ビューア

画像・動画のメタデータ（EXIF / XMP / IPTC / QuickTime など）を確認・編集する Windows 用ツールです。
インストール不要で、`exif-app.exe` 1 ファイルだけで動きます。

![アイコン](app-icon.svg)

## できること

- **見る**: 画像・動画をドラッグ&ドロップ（または「ファイルを開く」）すると、すべてのタグをグループごとに一覧表示します。検索で絞り込めます。
- **プレビュー**: JPEG・PNG などはそのまま、MP4・MOV は再生、RAW・HEIC はファイル内の埋め込みプレビューを表示します。
- **よく使う項目を編集**（右側のパネル）: 撮影日時（タイムゾーン付き）、タイトル、説明、作成者、著作権、キーワード、位置情報。
  画像と動画でタグの置き場所が違っても、アプリが適切なタグに書き込みます。
- **どのタグでも編集**: 一覧の値をダブルクリックで編集、🗑 で削除、下の欄で新しいタグを追加。
- **位置情報**: 「地図」タブ（OpenStreetMap）でクリックするかピンをドラッグして設定。数値でも入力できます。
- **メタデータ削除**: 位置情報だけ、またはすべてのメタデータを削除（画像の向きと色プロファイルは残します）。
- **書き出し**: メタデータを JSON / CSV（Excel で開ける形式）で保存。

## 使い方

1. `exif-app.exe` をダブルクリックして起動します（初回は準備に数秒かかります）。
   画像や動画を exe のアイコンにドロップして起動することもできます。
2. ファイルをウィンドウにドロップします。
3. 右のパネルやタグ一覧で編集します。変更はすぐにはファイルに書き込まれず、一覧に ● が付きます。
4. 「保存」（Ctrl+S）で、変更内容を確認してから書き込みます。
   - 初期設定では元のファイルを `ファイル名_original` として残します（2 回目以降の保存では最初のバックアップがそのまま残ります）。
   - 「別名で保存」なら、元のファイルは変えずにコピーに書き込みます。

### 対応形式

| 種類 | 形式 | 編集 |
|---|---|---|
| 画像 | JPEG, PNG, TIFF, WebP, HEIC/HEIF, AVIF, GIF | ○ |
| RAW | DNG, CR2, CR3, NEF, ARW, RAF, ORF, RW2 など | ○ |
| 動画 | MP4, MOV, M4V, 3GP | ○ |
| 動画 | AVI, MKV, WebM | 閲覧のみ |

## 注意

- **Windows SmartScreen**: コード署名をしていないため、初回起動時に「Windows によって PC が保護されました」と表示されることがあります。
  「詳細情報」→「実行」で起動できます。
- **必要なもの**: Windows 10 / 11（64bit）と Microsoft Edge WebView2 ランタイム（Windows 11 には標準で入っています）。
- **インターネット接続**: 地図の表示にだけ使います（OpenStreetMap のタイル）。オフラインでも地図以外はすべて使えます。
- **アプリが作るファイル**: exe の横には何も作りません。初回起動時に `%LOCALAPPDATA%\com.local.exifapp\` へ
  ExifTool を展開し、設定もそこに保存します。アプリを削除するときは、このフォルダも削除してください。
- 大切なファイルを編集する前には、念のためバックアップを取ってください。

## ライセンス

同梱・利用しているソフトウェアのライセンスは、アプリの「このアプリについて」で全文を確認できます。

- [ExifTool](https://exiftool.org/)（Phil Harvey）: Perl Artistic License / GPL
- 地図データ: © [OpenStreetMap](https://www.openstreetmap.org/copyright) contributors（ODbL）

---

## 開発者向け

設計と進捗は [docs/design.md](docs/design.md) にあります。

### 必要なもの
- Node.js 20 以上
- Rust (stable, 1.85 以上)

### コマンド

```powershell
npm install
npm run app:dev      # 開発モードで起動
npm test             # 画面側の単体テスト (vitest)
npm run build:exe    # 単体 exe をビルド → src-tauri/target/release/exif-app.exe
```

`app:dev` と `build:exe` は、最初に次の 2 つを自動で行います。

- `scripts/fetch-exiftool.mjs`: ExifTool（Windows 64bit 版）を取得し、exiftool.org の公式チェックサムで検証して `src-tauri/vendor/` に置く
- `scripts/collect-licenses.mjs`: exe に含まれる npm パッケージ・Rust クレート・ExifTool・OpenStreetMap のライセンスを集めて `src/generated/licenses.json` を作る

Rust 側のテストは、書き込んでよいテスト用ファイルを環境変数で渡したときだけ実ファイルを使って動きます。

```powershell
cd src-tauri
$env:EXIF_SAMPLE = "C:\path\to\sample.jpg"          # 読み取りテスト
$env:EXIF_WRITE_SAMPLE = "C:\path\to\copy.jpg"       # 特殊文字の書き込みテスト（上書きされます）
$env:EXIF_WRITE_DIR = "C:\path\to\dir"               # 写真.jpg / 動画.mp4 / 動画.mkv のコピーを置いたフォルダ（上書きされます）
cargo test --lib
```
