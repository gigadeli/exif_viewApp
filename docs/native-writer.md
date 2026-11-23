# 設計レポート: AVI / MKV / WebM のメタデータ書き込み（自作ライター）

作成日: 2026-11-23

## 1. 背景と目的

同梱している ExifTool 13.64 は AVI（RIFF）、MKV、WebM（Matroska）を**読み取ることはできるが、書き込みには対応していない**（`-listwf` にこれらの拡張子がない。`WriteRIFF.pl` が書けるのは WebP だけ）。

そこでこの 3 形式は、ExifTool を使わず Rust で書いた専用の書き込み処理（以下「自作ライター」）でタグを書き換える。読み取りは今までどおり ExifTool で行う。

検討した案:

| 案 | 長所 | 短所 | 判断 |
|---|---|---|---|
| ffmpeg を同梱してリマックス | 実装が簡単、形式に詳しい | exe が 80〜140MB 増える。ファイル全体を作り直す（遅い・空き容量が要る）。muxer 名・索引・タイムスタンプなどタグ以外も変わる | 不採用（「exe の軽さ」優先の方針に合わない） |
| **Rust で自作** | exe はほぼ増えない。タグ部分だけを書き換え、映像・音声データには触れない | 実装量が多い。特殊なファイルは「書き込めない」エラーになり得る | **採用** |

## 2. 方針

1. **映像・音声データは動かさない**。タグを置く「ヘッダー部分」の中だけで並べ替え、余った分は空き領域（AVI の `JUNK`、Matroska の `Void`）で埋める。データの位置が変わらないので、索引（AVI の `idx1` / OpenDML 索引、Matroska の `Cues`）を直す必要がない。
2. ヘッダー部分に入りきらない場合だけ、形式ごとの代わりの方法を使う（3.3、4.4）。それでも無理なら**何も変更せずにエラー**にする。
3. 書き込みは必ず**作業用コピーで行い、成功したら置き換える**（同じフォルダ内でコピー → 編集 → 名前の変更）。途中で失敗しても元ファイルは壊れない。
4. 書き込み後の検証は ExifTool で読み戻して行う（テスト）。アプリの「保存後に読み直す」流れも今までどおり。
5. 「バックアップを残す」「更新日時を保つ」の設定は ExifTool の場合と同じように動かす（`<名前>_original` を残す、更新日時を元に戻す）。

## 3. AVI（RIFF）

### 3.1 構造

```
RIFF <size> 'AVI '
  LIST <size> 'hdrl'   … ヘッダー（ストリーム情報。OpenDML の indx を含むことがある）
  LIST <size> 'INFO'   … タグ（無いこともある）
  JUNK <size>          … 空き領域（無いこともある）
  LIST <size> 'movi'   … 映像・音声データ
  idx1 <size>          … 索引
RIFF <size> 'AVIX' …   … 1GB を超える場合の続き（OpenDML）
```

- ExifTool は `LIST movi` に達したところで読み取りをやめる。そのため、**INFO は movi より前に置く必要がある**（末尾に追加しても読まれない）。
- INFO は `LIST 'INFO'` の中に 4 文字 ID のチャンク（`INAM` = タイトルなど）が並ぶ形。値は NUL 終端の文字列で、チャンクは偶数バイトに揃える。

### 3.2 書き込み（通常）

1. 最初の RIFF の、`'AVI '` の直後から `LIST movi` の手前までを「ヘッダー領域」とする。
2. ヘッダー領域から古い `LIST INFO` と `JUNK` を取り除き、それ以外のチャンク（`hdrl` など）は順番を保ったまま残す。
3. 変更を反映した新しい `LIST INFO` を `hdrl` の直後に置き、残りを `JUNK` で埋める。
   - 残りが 1〜7 バイトで JUNK を作れない場合は、INFO の最後の値の後ろに NUL を足して吸収する（読み取り側は末尾の NUL を無視する）。
4. ヘッダー領域の長さは変わらないので、ファイル内の他の位置は一切変わらない。

### 3.3 入りきらない場合（ヘッダー領域を広げる）

ヘッダー領域を `Δ` バイト広げ、それ以降を全部後ろにずらしてファイルを書き直す。`Δ` は 2048 の倍数にする（セクタ境界に揃えたファイルの配置を保つため）。余った分は JUNK として残すので、次回以降はその場で書き換えられる。

ずらすと狂う「ファイル先頭からの絶対位置」を直す:

- 最初の `RIFF` のサイズに `Δ` を足す（4GB を超えるならエラー）。
- OpenDML のスーパー索引（`hdrl` 内の `indx`、種類 `AVI_INDEX_OF_INDEXES`）の各エントリ `qwOffset` に `Δ` を足す。
- そのエントリが指す標準索引（`ix##`）の `qwBaseOffset` に `Δ` を足す。
- `idx1` の位置が絶対位置で書かれている場合（最初のエントリが `movi` の位置以上）は、全エントリに `Δ` を足す。`movi` からの相対位置（一般的）ならそのまま。

### 3.4 タグの対応

ExifTool の表示名（グループ `RIFF`）と INFO の ID:

| 表示名 | ID | 表示名 | ID |
|---|---|---|---|
| Title | INAM | Keywords | IKEY |
| Artist | IART | Genre | IGNR |
| Comment | ICMT | Subject | ISBJ |
| Copyright | ICOP | Software | ISFT |
| DateCreated | ICRD | Product | IPRD |
| Source | ISRC | Engineer | IENG |
| Technician | ITCH | Language | ILNG |
| Country | ICNT | Rating | IRTD |
| Starring | ISTR | Medium | IMED |
| ArchivalLocation | IARL | Commissioned | ICMS |

- 日付（ICRD）は `YYYY-MM-DD HH:MM:SS` で書く（ExifTool は `-` を `:` に変えて表示する）。
- **文字コード**: 値は UTF-8 で書く（ffmpeg など今のツールと同じ）。ExifTool は RIFF の文字列を既定で Latin-1 として読むので、読み取り時に INFO の値がすべて正しい UTF-8 なら `-api CharsetRIFF=UTF8` を付けて読む。正しい UTF-8 でない古いファイルは今までどおり読む。
- 古い AVI の INFO を変更するときは、既存値を ExifTool の Latin（Windows-1252）と同じ対応表で UTF-8 に変換する。値が変わらない保存では元のバイト列を保持する。
- `hdrl` 内の `IDIT`（DateTimeOriginal）など、INFO 以外のタグは書き込み対象外（読み取り専用）。

## 4. MKV / WebM（Matroska）

### 4.1 構造

EBML（ID + 可変長サイズ + データ）の入れ子構造。

```
EBML ヘッダー
Segment <size（不明もあり）>
  SeekHead     … 主要な要素の位置（Segment のデータ先頭からの相対位置）
  Void         … 空き領域
  Info         … Title, DateUTC, MuxingApp …
  Tracks
  Tags         … Tag { Targets, SimpleTag { TagName, TagString } … }
  Cluster …    … 映像・音声データ
  Cues         … 索引（Cluster の位置を持つ）
  Tags / SeekHead（末尾に置かれることもある）
```

- ExifTool は**最初の Cluster で読み取りをやめる**。ただし SeekHead に、それより後ろの Tags の位置が書かれていれば、そこへ飛んで読む。
- ffmpeg（MKV）は Info・Tags などの最初の子に `CRC-32` を付けることがある。中身を書き換えたら CRC を計算し直す。

### 4.2 書き換える要素

| ExifTool の表示（グループ:名前） | 書き換える場所 |
|---|---|
| `Info:Title` | Info / Title |
| `Info:DateTimeOriginal` | Info / DateUTC（2001-01-01 からのナノ秒、UTC） |
| `Matroska:<名前>` | 全体向けの Tag（Targets にトラックなどの UID を持たないもの）の SimpleTag（例: `Matroska:Artist` → `ARTIST`） |

- SimpleTag 名は ExifTool の StdTag 表の逆引き（Title→TITLE、DateTimeOriginal→DATE_RECORDED、CreateDate→DATE_DIGITIZED など）。
- 値を変える場合: 同じ名前の SimpleTag の TagString を置き換える（言語などの属性は残す）。無ければ全体向け Tag に追加し、全体向け Tag が無ければ `Targets`（空 = TargetTypeValue 50）付きで作る。
- 削除する場合: 全体向け Tag から、その名前の SimpleTag を消す。
- 言語違いなどで同名の全体向け SimpleTag が複数あるとき、個別の変更・削除は対象を特定できないため保存前にエラーにする。
- トラックごとのタグ（`Track1:Encoder` など）や EBML ヘッダー、Tracks の内容は書き込み対象外。

### 4.3 書き込み（通常）

1. Segment の子要素を順に読み、最初の Cluster の手前までを「ヘッダー領域」とする。
2. ヘッダー領域の要素（SeekHead、Info、Tracks、Tags、Chapters、Attachments など）を、Void を除いて元の順に並べ直す。Info と Tags は変更後の内容にし、最後に Void を置いて領域の長さを元どおりにする。
   - 残りが 1 バイトで Void を作れない場合は、直前の要素のサイズ欄を 1 バイト長く書いて吸収する（EBML はサイズ欄の長さを自由に選べる）。
3. ヘッダー領域内で位置が変わった要素については、SeekHead の SeekPosition を新しい位置に直す。SeekHead の大きさが変わると後ろの位置も変わるので、位置が落ち着くまで並べ直しを繰り返す。
4. Cluster 以降は動かないので、Cues などは直さない。

### 4.4 入りきらない場合

次の順に試し、どれもできなければ**何も変えずにエラー**にする。

1. **Tags を末尾へ移す**: 変更した Tags（新しく作った Tags も含む）をヘッダー領域から取り除き、Segment の末尾に置く。SeekHead にはその位置を書く（ExifTool やプレーヤーは SeekHead から見つける）。
   - Segment がファイルの最後で終わっている場合だけ行う。Segment のサイズが決まっていればサイズ欄を書き直す（サイズ欄の長さは変えない）。サイズが「不明」ならそのまま。
   - ヘッダー領域に SeekHead が無い場合は、新しく作ってヘッダー領域の先頭に置く。
2. **ヘッダー領域を広げる**（実装中に追加。長いタイトルなどで Info が大きくなった場合に必要）: 足りない分に 4KB の余白を足した `Δ` バイトだけヘッダー領域を広げ、ファイルを書き直す。Cluster 以降はすべて `Δ` だけ後ろへずれるので、次を直す。
   - SeekHead が指す、Cluster より後ろの要素の位置
   - Cues の `CueClusterPosition`（と、0 でない `CueCodecState`）。元と同じバイト数で書き、Cues 自体はその場に収める
   - Cluster の中の `Position`（任意の要素。元と同じバイト数に収まらなければエラー）
   - Segment のサイズ

- 元から Tags が末尾にあるファイル（Cluster より後）は、それがファイルの最後の要素ならその位置に書き直す（長さが変わってよい）。そうでなければ、その場（後ろの Void も含む）に入れば書き換え、入らなければ古い方を Void にして末尾に新しく置き、SeekHead を直す。

### 4.5 その他

- サイズ不明の Cluster（ライブ配信の WebM など）は、子要素を読んで終わりを見つける（Segment 直下の ID が出てきたら終わり）。
- ファイル内に Segment が複数ある場合（連結ファイル）は、最初の Segment だけを対象にし、末尾への追加はできない（エラー）。
- 書き込める型（拡張子ではなく中身で判定）: EBML の DocType が `matroska` または `webm`。

## 5. 「すべてのメタデータを削除」

- AVI: `LIST INFO` をすべて取り除く（JUNK に置き換え）。
- Matroska: Info の Title・DateUTC と、Tags 要素をすべて取り除く（トラックの統計タグも含む）。

## 6. アプリへの組み込み

- Rust: `src-tauri/src/native/`
  - `mod.rs`: 形式の判定、変更の振り分け、書き込み可能なタグの一覧、作業用コピーへの書き込みと置き換え
  - `riff.rs`: AVI
  - `ebml.rs`: EBML の読み書き（可変長整数、要素の読み取り・組み立て）
  - `matroska.rs`: MKV / WebM
- `write_metadata` と `save_as` は、ファイルの先頭のバイト列で AVI または Matroska と判定したら自作ライターを使い、それ以外は今までどおり ExifTool を使う。
- `writable_info` に、自作ライターが書ける FileType（`AVI` / `MKV` / `WEBM`）と、グループごとの書き込めるタグ名を加える。フロントエンドはそれを見て、タグ表の編集可否と「タグを追加」の候補を決める。
- 「よく使う項目」の対応:

| 項目 | AVI | MKV / WebM |
|---|---|---|
| 撮影日時 | `RIFF:DateCreated`（ローカル時刻、タイムゾーンなし） | `Info:DateTimeOriginal`（UTC で保存、PC のタイムゾーンで表示）。`Matroska:DateTimeOriginal` があれば同じ日時に揃える |
| タイトル | `RIFF:Title` | `Info:Title` と `Matroska:Title`（あれば） |
| 説明 | `RIFF:Comment` | `Matroska:Description`（`Matroska:Comment` があれば揃える） |
| 作成者 | `RIFF:Artist` | `Matroska:Artist` |
| 著作権 | `RIFF:Copyright` | `Matroska:Copyright` |
| キーワード | `RIFF:Keywords` | `Matroska:Keywords` |
| 位置情報 | 非対応（欄を無効にする） | 非対応（欄を無効にする） |

## 7. テスト

ウィンドウを出さずに確認する（Rust のテストで書き込み、ExifTool で読み戻し、ffmpeg でデータを比べる）。

- テスト用の動画は `python scripts/make-native-samples.py <フォルダ>` で作る（ffmpeg が必要。開発時だけで、アプリには同梱しない）。
  | ファイル | 内容 |
  |---|---|
  | plain.avi / noinfo.avi | INFO あり / ソフト名だけの AVI |
  | plain.mkv | CRC-32 付きの Info・Tags、SeekHead、Cues |
  | plain.webm | WebM（CRC なし） |
  | stream.webm / stream.mkv | パイプへの出力（Segment のサイズ不明、Cues なし） |
  | unknown_cluster.webm | Cluster のサイズも不明（ライブ配信の形） |
  | noseek.mkv | SeekHead なし |
  | tailtags.mkv | Tags が Cues より後ろ（ファイルの最後） |
- 1GB を超える AVI（OpenDML の索引と AVIX を持つ）は大きいので別に作る:
  `ffmpeg -f lavfi -i testsrc=size=640x480:rate=25 -f lavfi -i sine -t 100 -c:v rawvideo -pix_fmt yuv420p -c:a pcm_s16le odml.avi`
- 実行: `EXIF_NATIVE_DIR=<フォルダ>`（と `EXIF_NATIVE_BIG=<odml.avi>`）を設定して `cargo test --lib native`。
- 確認していること（`src-tauri/src/native/file_tests.rs`）:
  - 書いた値を ExifTool で読み戻せる（日本語、改行、数万文字の値、日時、キーワード）。ExifTool の警告が出ない
  - 書けないタグは警告になり、他の変更は書き込まれる
  - タグの削除、すべて削除、削除後の追加。同じ値なら書き換えない
  - 入りきらない場合の経路: AVI のヘッダー拡張（2048 の倍数で広がり、次回はその場に書ける）、MKV の Tags の末尾への移動、MKV のヘッダー拡張
  - 映像・音声データが変わっていない（ffmpeg `-c copy -f framemd5` のパケットの MD5 が書き込みの前後で同じ）
  - ffmpeg がエラーなしで最後まで読める。1GB 超の AVI は途中へのシークもできる
  - MKV の SeekHead と Cues の位置が、すべて正しい要素（Cues は Cluster）を指している
  - OpenDML のスーパー索引が ix## を指し、ix## のエントリが映像・音声のチャンクを指している
  - バックアップ（`_original` は元ファイルと同じ内容）、更新日時を変えない設定、作業用ファイルが残らない

## 8. 結果と制限

- 上の確認はすべて合格（2026-11-23）。exe のサイズは 15.5MB → 15.6MB。
- 画面（よく使う項目・タグ表）は、モックの API を使ってブラウザで表示し、AVI / MKV で書き込めるタグだけが編集でき、
  位置情報の欄が無効になり、保存すると想定どおりの変更が送られることを確認した。実際の exe のウィンドウでの操作は未確認。
- 制限:
  - 位置情報は書き込めない（AVI には標準の置き場所が無い。Matroska の `RECORDING_LOCATION` などは読み手がほとんど無い）。
  - トラックごとのタグ、AVI の `IDIT`（hdrl 内の撮影日時）、EBML ヘッダー・Tracks の内容は読み取り専用。
  - AVI の INFO は UTF-8 で書くため、古いソフト（Windows のエクスプローラーのプロパティなど）では日本語が文字化けする場合がある。
  - MKV のヘッダー拡張で Cues や Cluster の Position の桁が増える場合、Segment が複数ある連結ファイル、
    途中で壊れているファイルは、何も変えずにエラーになる。
  - mkvmerge など ffmpeg 以外のソフトで作ったファイルは、手元に無いため未確認。
