# EXIF Viewer / Editor 設計書 (v0.1 ドラフト)

画像・動画のメタデータ（EXIF / XMP / IPTC / QuickTime など）を確認・編集できる Windows 向けデスクトップツール。
ユーザーには単一の `.exe`（インストーラまたはポータブル）として配布する。

---

## 1. 目的とスコープ

### 1.1 やりたいこと
- 画像・動画を開いて、メタデータを一覧で確認する
- よく使う項目（撮影日時、GPS、カメラ情報、作成者、説明など）を編集する
- メタデータの一括削除（プライバシー目的：GPS だけ消す / 全部消す）
- 複数ファイルへの一括編集（撮影日時のずらし、作成者の一括設定など）

### 1.2 MVP（最初のリリース）に含めるもの
| 機能 | 内容 |
|---|---|
| ファイルを開く | ドラッグ&ドロップ / ファイル選択ダイアログ / 複数選択 |
| 一覧表示 | グループ別（EXIF, XMP, IPTC, QuickTime, File...）のタグ表。検索フィルタ付き |
| プレビュー | 画像サムネイル、動画は先頭フレーム or `<video>` 再生 |
| 編集 | 「よく使う項目」フォームで編集 + 詳細タブで任意タグを直接編集 |
| GPS | 地図表示（Leaflet + OpenStreetMap）、地図クリックで位置設定、GPS 削除 |
| 削除 | 全メタデータ削除 / GPS のみ削除 |
| 保存 | 上書き保存（初回はバックアップ `_original` を残す設定可）/ 別名保存 |
| エクスポート | メタデータを JSON / CSV で書き出し |

### 1.3 MVP 後（v2 以降）
- 一括編集（複数ファイルへの同時適用、日時シフト、連番リネームとの組み合わせ）
- 編集テンプレート（プリセット）の保存
- 変更前後の差分表示、Undo 履歴

UI は日本語のみ（多言語化はしない。ただし文言は 1 ファイルにまとめておく）。

### 1.4 スコープ外
- 画像の画素データの編集（回転・トリミング等）
- 動画の再エンコード

---

## 2. 対応フォーマット

メタデータの読み書きは **ExifTool** に任せる。対応範囲は ExifTool の能力に準じる。

| 種別 | 主な形式 | 読み | 書き |
|---|---|---|---|
| 画像 | JPEG, PNG, TIFF, WebP, HEIC/HEIF, AVIF | ○ | ○ |
| RAW | CR2/CR3, NEF, ARW, DNG, RAF など | ○ | ○（DNG/主要 RAW） |
| 動画 | MP4, MOV, M4V, 3GP | ○ | ○（QuickTime/XMP タグ） |
| 動画 | AVI, MKV/WebM | ○ | ○（ExifTool は非対応のため自作ライター。[native-writer.md](native-writer.md)） |
| その他 | GIF, BMP | ○ | △ |

書き込み可否はファイル読み込み時に ExifTool の結果から判定し、不可なら編集 UI を無効化する。

---

## 3. 技術選定

### 3.1 推奨構成：Tauri 2 + React/TypeScript + ExifTool（サイドカー）

```
┌──────────────────────── exif-app.exe (Tauri) ────────────────────────┐
│  Frontend (WebView2)                 Backend (Rust)                   │
│  React + TypeScript + Vite   ◀─IPC─▶ Tauri commands                   │
│  - ファイル一覧 / タグ表          │  - ExifTool プロセス管理          │
│  - 編集フォーム / 地図            │  - バックアップ・保存処理         │
│  - プレビュー                     │  - 設定の永続化                   │
└──────────────────────────────────────────────┬───────────────────────┘
                                               │ stdin/stdout (-stay_open)
                                       ┌───────▼────────┐
                                       │ exiftool.exe   │ (同梱サイドカー)
                                       └────────────────┘
```

**選定理由**
- **ExifTool**：画像・動画・RAW を 1 つで読み書きできる事実上の標準。自前で各フォーマットのパーサを書くのは非現実的（特に動画・HEIC・メーカーノート）。Windows 用スタンドアロン版があり再配布可（Perl Artistic / GPL のデュアルライセンス）。
- **Tauri**：exe サイズが小さい（Electron の 100MB 超に対して 10MB 前後）、Windows 11 は WebView2 標準搭載。UI を Web 技術で書けて地図（Leaflet）等の資産も使える。
- **Rust バックエンド**：ExifTool プロセスの常駐管理・ファイル操作を安全に書ける。

### 3.2 比較した代替案
| 案 | 長所 | 短所 |
|---|---|---|
| Electron + exiftool | 情報が多い、Node だけで完結 | exe が重い（100MB+）、メモリ消費大 |
| Python + PySide6 + PyInstaller | 実装が速い | exe が重め、起動が遅い、アンチウイルス誤検知が多い |
| C# WPF/WinUI + exiftool | Windows ネイティブ | UI 実装の手間、地図などの資産が少ない |
| Rust 純正（kamadak-exif 等） | 依存なし | 動画・HEIC・XMP の**書き込み**が弱く要件を満たせない |

---

## 4. アーキテクチャ詳細

### 4.1 ExifTool 連携
- 起動時に `exiftool -stay_open True -@ -` で 1 プロセスを常駐させ、コマンドを stdin で送り `{ready}` で区切って結果を受け取る（ファイルごとにプロセス起動するより 10〜50 倍速い）。
- 読み取り：`-json -G1 -a -s -n` （グループ付き・重複タグ含む・数値生値）と、表示用に `-json -G1`（人間可読値）の 2 種を取得し、表示値と生値の両方を保持。
- 書き込み：`-TAG=value` を引数として渡す。シェルを介さず引数配列で渡すので、値のエスケープ問題を避けられる。
- 文字コード：`-charset filename=utf8` を指定し、日本語パスを扱えるようにする。
- タイムアウト・クラッシュ時はプロセスを再起動。

### 4.2 Tauri コマンド（フロント ⇔ バックエンド API）
```ts
read_metadata(paths: string[]): FileMetadata[]
write_metadata(path: string, changes: TagChange[], options: WriteOptions): WriteResult
remove_metadata(path: string, scope: "all" | "gps" | TagGroup[]): WriteResult
export_metadata(paths: string[], format: "json" | "csv", dest: string): void
get_thumbnail(path: string): string   // base64 or asset URL
get_settings() / set_settings(s)
```

### 4.3 データモデル
```ts
type FileMetadata = {
  path: string;
  fileName: string;
  mimeType: string;
  writable: boolean;            // 書き込み可能な形式か
  tags: Tag[];
};

type Tag = {
  group: string;                // "EXIF:IFD0", "XMP-dc", "QuickTime" など (-G1)
  name: string;                 // "DateTimeOriginal"
  value: string;                // 表示用 (人間可読)
  raw: string | number;         // 生値 (-n)
  writable: boolean;
};

type TagChange = { group?: string; name: string; value: string | null }; // null = 削除

type WriteOptions = {
  backup: boolean;              // false なら -overwrite_original
  preserveFileTime: boolean;    // -P (更新日時を保持)
};
```

### 4.4 「よく使う項目」フォームとタグの対応
動画と画像で格納先タグが異なるため、論理項目 → 実タグへのマッピング表を持つ。

| 論理項目 | 画像 (JPEG 等) | 動画 (MP4/MOV) |
|---|---|---|
| 撮影日時 | EXIF:DateTimeOriginal, CreateDate, OffsetTime* | QuickTime:CreateDate, MediaCreateDate, TrackCreateDate（UTC 保存に注意） |
| GPS | EXIF:GPSLatitude/Longitude/Altitude (+Ref) | QuickTime:GPSCoordinates, XMP:GPS* |
| タイトル / 説明 | XMP-dc:Title, Description, EXIF:ImageDescription | QuickTime:Title, Description |
| 作成者 / 著作権 | EXIF:Artist, Copyright, XMP-dc:Creator, Rights | QuickTime:Artist, Copyright |
| キーワード | XMP-dc:Subject, IPTC:Keywords | XMP-dc:Subject |
| カメラ（参照のみ） | Make, Model, LensModel, 露出情報 | Make, Model（あれば） |

※ QuickTime の日時は仕様上 UTC。ExifTool の `-api QuickTimeUTC` を使い、UI ではローカル時刻で表示・入力させる。

### 4.5 保存の安全性
- 書き込みは ExifTool が一時ファイル経由で行うため、途中失敗で元ファイルは壊れない。
- 設定「バックアップを残す」ON（初期値）で `xxx.jpg_original` を残す。OFF で `-overwrite_original`。
- 未保存の変更があるままファイルを閉じる / アプリ終了時は確認ダイアログ。
- 書き込み前に変更内容のサマリ（何タグをどう変えるか）を表示する。

---

## 5. 画面設計

```
┌───────────────────────────────────────────────────────────────────┐
│ [開く] [保存] [別名保存] [メタデータ削除▼] [エクスポート▼]   [⚙設定] │
├──────────────┬──────────────────────────────┬─────────────────────┤
│ ファイル一覧  │ プレビュー                    │ よく使う項目（編集） │
│ ┌──────────┐ │ ┌──────────────────────────┐ │ 撮影日時 [______]   │
│ │🖼 a.jpg ●│ │ │                          │ │ タイトル [______]   │
│ │🎞 b.mp4  │ │ │      画像 / 動画          │ │ 説明     [______]   │
│ │🖼 c.heic │ │ │                          │ │ 作成者   [______]   │
│ └──────────┘ │ └──────────────────────────┘ │ 著作権   [______]   │
│ (● = 未保存)  ├──────────────────────────────┤ キーワード [tag][+] │
│              │ [全タグ] [GPS/地図]            │ GPS  35.68,139.76  │
│ ここにドロップ │ 検索: [________]              │      [地図で設定]    │
│              │ Group      │ Tag      │ Value │      [GPS削除]      │
│              │ EXIF:IFD0  │ Make     │ Canon │                     │
│              │ EXIF:ExifIFD│ FNumber │ 2.8 ✎ │ カメラ: Canon EOS R6 │
│              │ ...                            │ レンズ: RF24-70     │
└──────────────┴──────────────────────────────┴─────────────────────┘
│ ステータスバー: 3 ファイル / 1 件未保存 / ExifTool 13.xx           │
└───────────────────────────────────────────────────────────────────┘
```

- **全タグ表**：グループで折りたたみ、セルのダブルクリックで直接編集（書き込み可能タグのみ）、行ごとに削除ボタン。変更されたセルはハイライト。
- **GPS/地図タブ**：現在位置にピン表示、クリック / ピンのドラッグで座標変更。OSM タイルはオンライン時のみ（オフラインでも座標の数値入力は可能）。
- **ファイルは複数開けるが、編集対象は常に 1 ファイル**（一覧で選択中のもの）。複数ファイルへの同時編集は v2。
- 外観は Windows 11 のライト / ダークに追従。

---

## 6. ディレクトリ構成（予定）

```
exif_app/
├─ docs/design.md
├─ src/                      # フロントエンド (React + TS)
│  ├─ components/            # FileList, Preview, TagTable, QuickEditForm, MapView
│  ├─ hooks/                 # useMetadata, useDirtyState
│  ├─ lib/tagMapping.ts      # 論理項目 → 実タグ のマッピング
│  ├─ api.ts                 # Tauri invoke のラッパ
│  └─ App.tsx
├─ src-tauri/
│  ├─ src/
│  │  ├─ main.rs
│  │  ├─ commands.rs         # Tauri コマンド
│  │  ├─ exiftool.rs         # stay_open プロセス管理
│  │  └─ settings.rs
│  ├─ vendor/exiftool.zip    # exe に埋め込む ExifTool 一式 (exiftool.exe + exiftool_files/)
│  └─ tauri.conf.json
└─ package.json
```

---

## 7. 配布

**配布形式：インストール不要の単体 exe（`exif-app.exe` 1 ファイル）**

- `tauri build` でできる `target/release/exif-app.exe` をそのまま配布する（インストーラは作らない）。
- **ExifTool の埋め込み**：Windows 版 ExifTool は `exiftool.exe` と `exiftool_files/` フォルダの組なので、そのままでは 1 ファイルにならない。そこで次のようにする。
  1. ビルド時に ExifTool 一式を zip にして、Rust の `include_bytes!` で exe に埋め込む。
  2. 起動時に `%LOCALAPPDATA%\com.local.exifapp\exiftool\<バージョン>-<ハッシュ>\` へ展開する（既に展開済みでハッシュが一致すれば何もしない）。
  3. 展開先の `exiftool.exe` を `-stay_open` で起動する。
  - 展開は初回だけなので、2 回目以降の起動速度には影響しない。exe サイズは 20MB 前後の見込み。
- **設定ファイル**も `%LOCALAPPDATA%\com.local.exifapp\settings.json` に置く（exe の横には何も作らない）。
- ExifTool のライセンス表記を「このアプリについて」画面に含める。
- WebView2 は Windows 11 に標準搭載なので同梱しない。万一無い環境では、起動時にダウンロードページへの案内を出す。
- コード署名なしだと SmartScreen 警告が出る点は README に記載（必要になれば署名を検討）。

**ネットワーク**：地図タイルの取得（OpenStreetMap）にのみオンライン接続を使う。OSM のタイル利用規約に従い、帰属表示（© OpenStreetMap contributors）を地図に表示する。オフライン時は地図部分に「オフラインのため地図を表示できません」と出し、座標の数値入力は使える。

---

## 8. 開発ステップ

1. **土台**：Tauri + React + TS のプロジェクト作成、ExifTool の埋め込み・展開と `-stay_open` 連携、1 ファイルの全タグ表示
2. **閲覧機能**：複数ファイル、D&D、プレビュー、検索、JSON/CSV エクスポート
3. **編集機能**：全タグ表の直接編集、保存 / バックアップ、未保存管理
4. **よく使う項目フォーム**：画像/動画のタグマッピング、日時・タイムゾーン処理
5. **GPS**：地図表示・設定・削除、メタデータ一括削除
6. **配布**：単体 exe のビルド、ExifTool の初回展開の確認、ライセンス表記、テスト用サンプル（JPEG/HEIC/MP4/MOV/RAW）で動作確認
7. **v2**：一括編集、プリセット、差分表示

---

## 9. 決定事項

| 項目 | 決定 |
|---|---|
| 技術スタック | Tauri 2 + React/TS + ExifTool 同梱（exe の軽さ・起動速度を重視、TS/React に慣れているため）。Rust 側は ExifTool のプロセス管理とファイル操作だけの薄い層に抑え、ロジックの大半は TypeScript 側に置く |
| 配布形式 | インストール不要の単体 exe（ExifTool は埋め込み、初回起動時に展開） |
| 一括編集 | MVP には入れない（v2） |
| UI 言語 | 日本語のみ |
| 地図 | オンラインの OpenStreetMap タイルを使う（帰属表示あり） |

---

## 10. 進捗

| ステップ | 状態 | メモ |
|---|---|---|
| 1. 土台 | 完了 | ExifTool 13.64 を埋め込み。初回起動時に展開、終了時に停止 |
| 2. 閲覧機能 | 完了 | プレビュー、JSON/CSV 書き出し、起動引数で渡したファイルを開く |
| 3. 編集機能 | 完了 | タグ表での直接編集・削除・追加、上書き保存 / 別名で保存、バックアップ、未保存の管理 |
| 4. よく使う項目フォーム | 完了 | 右側パネル。撮影日時（タイムゾーン付き）、タイトル、説明、作成者、著作権、キーワード、撮影情報（参照のみ） |
| 5. GPS | 完了 | 地図タブ（OpenStreetMap）、右パネルの緯度・経度・高度、位置情報の削除、すべてのメタデータの削除 |
| 6. 配布 | 完了 | 単体 exe（15.5MB）、アイコン、exe のバージョン情報、「このアプリについて」（ライセンス全文）、利用者向け README |
| 7. v2 | 未着手 | 一括編集、プリセット、差分表示 など |

### プレビューの対応範囲（ステップ 2 時点）
| 形式 | 表示方法 |
|---|---|
| JPEG, PNG, GIF, WebP, BMP, AVIF | WebView でそのまま表示（開いたファイルだけを asset プロトコルの許可範囲に追加） |
| MP4, M4V, MOV, WebM | `<video>` で再生。コーデック非対応（HEVC など）なら「再生できません」と表示 |
| RAW, HEIC など上記以外 | ExifTool でファイル内の埋め込みプレビュー（PreviewImage / JpgFromRaw / OtherImage / ThumbnailImage のうち最大のもの）を表示 |
| 埋め込みプレビューの無い TIFF、MKV、AVI など | 「この形式はプレビューできません」と表示 |

今後の改善候補: TIFF や埋め込みプレビューの無い HEIC を Windows の画像デコーダー (WIC) で表示する、動画の先頭フレームを表示する。

### 書き出し
- JSON: ExifTool の `-j -G1 -a -s` の出力そのまま（ファイルごとに 1 オブジェクト）
- CSV: ExifTool の `-csv -G1 -a -s` 形式（1 行 = 1 ファイル、列 = タグ）。Excel で文字化けしないよう UTF-8 BOM 付き

### 編集・保存（ステップ 3 時点）
- **編集できるタグ**: ExifTool の書き込み対応形式 (`-listwf`) で、書き込み可能なタグ名 (`-listw`) に含まれ、
  ExifTool / System / File / Composite グループ以外で、バイナリ値でないもの。判定はタグ名単位なので、
  まれに書き込めないものも編集できてしまう → その場合は保存時に ExifTool の警告を結果ダイアログに表示する。
- **操作**: 値をダブルクリック（または ✎）で編集。Enter で確定、Shift+Enter で改行、Esc で取り消し。
  編集欄からフォーカスが外れたときも確定する。🗑 で削除、↺ で元に戻す。下の欄で新しいタグを追加。
- **未保存の変更**はファイルごとに保持し、一覧に ● を表示。ファイルを外す・すべて閉じる・ウィンドウを閉じるときは確認する。
- **保存**（Ctrl+S）: 変更前後の一覧を確認してから書き込む。オプションは
  「元ファイルを `_original` として残す」（初期値 ON）と「更新日時を変えない（`-P`）」。選択は次回も引き継ぐ。
- **別名で保存**: 元ファイルをコピーしてから、コピーに変更を書き込む（元ファイルは変更しない）。書き込みに失敗したらコピーは消す。
- 改行を含む値の渡し方は下の「ExifTool への引数の渡し方」を参照。IPTC を書くときは `CodedCharacterSet=UTF8` を付ける。
- 設定（バックアップ等）は WebView の localStorage に保存する。実体は `%LOCALAPPDATA%\com.local.exifapp\EBWebView\` にあり、settings.json と同じく「exe の横には何も作らない」を満たすので、settings.json は作らないことにした。

### よく使う項目のタグ対応（ステップ 4 時点、実装は `src/fields.ts`）
各項目は「常に書くタグ」と「ファイルに既にあるときだけ同じ値に揃えるタグ」に書き込む。
揃えないと、古い値が別のタグに残って他のソフトで食い違って見えるため。表示は「読む順」の最初に値があるタグ。

| 項目 | 画像: 常に書く | 画像: あれば揃える | 動画: 常に書く | 動画: あれば揃える |
|---|---|---|---|---|
| タイトル | XMP-dc:Title | IPTC:ObjectName | ItemList:Title | Keys:Title, UserData:Title, XMP-dc:Title |
| 説明 | XMP-dc:Description, IFD0:ImageDescription | IPTC:Caption-Abstract | ItemList:Description | Keys:Description, UserData:Description |
| 作成者 | IFD0:Artist, XMP-dc:Creator | IPTC:By-line | ItemList:Artist | Keys:Artist, Keys:Author, UserData:Artist/Author |
| 著作権 | IFD0:Copyright, XMP-dc:Rights | IPTC:CopyrightNotice | ItemList:Copyright | Keys:Copyright, UserData:Copyright |
| キーワード | XMP-dc:Subject | IPTC:Keywords | XMP-dc:Subject | Keys:Keywords |

撮影日時:
- **画像**: ExifIFD:DateTimeOriginal と CreateDate にローカル時刻を書く。タイムゾーンを選ぶと
  OffsetTimeOriginal / OffsetTimeDigitized にも書き、「指定なし」にすると既存のものを消す。
  XMP の日時（XMP-exif:DateTimeOriginal など）は、あればタイムゾーン付きで揃える。
- **動画**: QuickTime:CreateDate と各トラックの TrackCreateDate / MediaCreateDate に **UTC** で書き、
  Keys:CreationDate（Apple 形式）にローカル時刻 + タイムゾーンで書く。
  表示は Keys:CreationDate を優先し、無ければ UTC の CreateDate を PC のタイムゾーンに直して表示する。
- 0000:00:00 00:00:00 は「日時なし」として扱う。

### ExifTool への引数の渡し方（ステップ 4 で見直し）
- 引数ファイルの通常の行では、ExifTool が `-タグ=` の直後の空白を 1 つ削るため、`=` の直後に空白を 1 つ足して渡す（値の前後の空白を保つ）。
- 改行は `-E` オプションと HTML 文字参照（`&#xa;`）で渡す。`&` は `&amp;` にする。
  `#[CSTR]` 形式は `$` `@` の前に `\` が付いてしまうため使わない。
- キーワードなどのリスト型タグは `-sep ", "` で分割して書く（表示も ", " 区切り）。
- 同じ項目を元の値に戻した変更は「変更なし」として自動で取り除く。

### 位置情報と削除（ステップ 5 時点、実装は `src/gps.ts`）
- **画像**: GPS:GPSLatitude / GPSLongitude に絶対値、GPSLatitudeRef / GPSLongitudeRef に N/S・E/W、
  高度は GPSAltitude + GPSAltitudeRef（Above / Below Sea Level）。XMP-exif:GPSLatitude / GPSLongitude は、あれば符号付きで揃える。
- **動画**: Keys:GPSCoordinates（Apple）と UserData:GPSCoordinates（©xyz、Google フォトなどが読む）の両方に "緯度, 経度[, 高度]" を書く。
- **位置情報の削除**: ファイルにある GPS 関連タグ（GPS グループ、名前が GPS で始まるタグ。Composite は除く）を 1 つずつ削除する変更を作る。
- **地図**: Leaflet + OpenStreetMap タイル。クリックまたはピンのドラッグで位置を設定。帰属表示はアプリ内で別ページに移動しないよう文字だけ。
  タイルを 1 枚も読めないときは「オフラインのため地図を表示できません」と表示する。
- **すべてのメタデータを削除**: 特別な変更（グループ空・名前 all・値なし）として保留し、保存時に
  `-all= -tagsFromFile @ -IFD0:Orientation -ICC_Profile` を最初に、その後に他の変更を書き込む。
  画像の向きと色プロファイルは見た目が変わるので残す。`--Orientation` での除外では向きが残らないことを確認済み。

### 配布（ステップ 6 時点）
- `npm run build:exe` → `src-tauri/target/release/exif-app.exe`（約 15.5MB、インストーラなし）。
  ビルド前に `scripts/fetch-exiftool.mjs`（ExifTool の取得と公式チェックサムの検証）と
  `scripts/collect-licenses.mjs`（ライセンスの収集）が自動で動く。
- exe のバージョン情報: 製品名・説明「EXIF Viewer」、バージョン 0.1.0、著作権表示。アイコンは `app-icon.svg` から `tauri icon` で生成。
- **ライセンス表記**: 「このアプリについて」に、exe に含まれる npm パッケージ（6）、Rust クレート（241、Windows 向けの通常の依存のみ）、
  ExifTool、OpenStreetMap のライセンス名と全文を表示する（同じ本文はまとめて 155 種類）。
  ライセンスファイルを同梱していないクレート（5 件）は、MIT / BSD-3-Clause は SPDX の標準文に Cargo.toml の著作者を入れ、
  MPL-2.0 は同じライセンスの他のクレートの本文を使う。ExifTool に含まれる Strawberry Perl のライセンスは、展開先の
  `exiftool_files/Licenses_Strawberry_Perl.zip` にある。
- **初回起動**: ExifTool の展開を含めて約 3 秒で使えるようになる（展開済みなら展開は省略）。

### 未確認・今後の課題
- HEIC / RAW の実ファイルでの確認（テスト用のファイルが無く、JPEG・MP4・MOV・MKV・TIFF でのみ確認）
- オフライン時の地図の表示（「オフラインのため地図を表示できません」）
- Windows 10、WebView2 が入っていない環境での動作
- コード署名（無いと SmartScreen の警告が出る。README に回避方法を記載）
- 地図の帰属表示を OSM の著作権ページへのリンクにする（外部ブラウザで開く仕組みが必要）
- TIFF や埋め込みプレビューの無い HEIC のプレビュー（Windows の画像デコーダーを使う）

### MVP 後の修正（2026-11-23）
- タグ一覧の罫線のずれを修正（値のセル自体を行数制限用の `-webkit-box` にしていたため表のレイアウトから外れていた。制限は内側の要素で行う）。
- **バイナリデータの表示**: `(Binary data … bytes)` のタグは値のリンクか 👁 で中身を表示。
  画像（JPEG / PNG / GIF / WebP / BMP）はそのまま、UTF-8 の文字列はテキスト、それ以外は 16 進ダンプ（先頭 64KB まで）。
  「ファイルに保存」でタグの中身をそのまま書き出す（Rust 側で ExifTool の `-j -b` の base64 を復号。実装は `src-tauri/src/binary.rs`）。
- **よく使う項目の非表示**: ツールバーの「よく使う項目を隠す / 表示」とパネル見出しの × で切り替え。状態は次回起動時も引き継ぐ。

### AVI / MKV / WebM の書き込み（2026-11-23）
- ExifTool はこの 3 形式に書き込めないため、Rust で自作した書き込み処理（`src-tauri/src/native/`）を使う。
  設計と検証の詳細は [native-writer.md](native-writer.md)。
- 書き込めるのは AVI の INFO（`RIFF:Title` など 20 種）、Matroska の `Info:Title` / `Info:DateTimeOriginal` と
  全体向けの SimpleTag（`Matroska:Artist` など）。トラックごとのタグと位置情報は対象外。
- 映像・音声データは動かさず、ヘッダー部分の空き領域（JUNK / Void）の中で並べ替える。足りなければ、
  MKV は Tags を末尾へ移す。それでも足りなければ（AVI はすぐに）ヘッダー部分を広げてファイルを書き直し、索引の位置を直す。
- AVI の INFO は UTF-8 で書く。読み取り時、INFO の値がすべて正しい UTF-8 なら ExifTool に `-api CharsetRIFF=UTF8` を付ける。
