// 画面の文言はここにまとめる

export const S = {
  appName: "EXIF ビューア",
  open: "ファイルを開く",
  clear: "すべて閉じる",
  dropHint: "画像や動画をここにドロップ",
  dropOverlay: "ドロップして開く",
  noFiles: "ファイルがありません",
  noSelection: "左の一覧からファイルを選んでください",
  search: "タグ名・値で検索",
  colGroup: "グループ",
  colTag: "タグ",
  colValue: "値",
  noMatch: "一致するタグがありません",
  loading: "読み込み中…",
  starting: "ExifTool を準備中…",
  remove: "一覧から外す",
  tagCount: (n: number) => `${n} 件のタグ`,
  fileCount: (n: number) => `${n} ファイル`,
  exiftool: (v: string) => `ExifTool ${v}`,
  warnings: "警告",
  readError: "読み取りエラー",
  dialogFilterMedia: "画像・動画",
  dialogFilterAll: "すべてのファイル",

  // プレビュー
  previewShow: "プレビューを表示",
  previewHide: "プレビューを隠す",
  previewLoading: "プレビューを読み込み中…",
  previewNone: "この形式はプレビューできません",
  previewEmbedded: "ファイル内の埋め込みプレビューを表示しています",
  previewVideoError: "この動画はプレビュー再生できません（コーデック非対応）",

  // 書き出し
  exportMenu: "書き出し",
  exportJsonCurrent: "選択中のファイルを JSON で",
  exportCsvCurrent: "選択中のファイルを CSV で",
  exportJsonAll: "開いているすべてを JSON で",
  exportCsvAll: "開いているすべてを CSV で",
  exportDone: (path: string) => `書き出しました: ${path}`,
  exportFailed: "書き出しに失敗しました",
  exportDefaultName: "metadata",

  // 編集
  edit: "編集",
  deleteTag: "削除",
  revert: "元に戻す",
  editHint: "Enter で確定 / Shift+Enter で改行 / Esc で取り消し",
  readOnlyFile: "この形式は ExifTool が書き込みに対応していないため、閲覧のみです",
  deleted: "（削除）",
  newGroup: "追加するタグ",
  addTag: "タグを追加",
  addTagName: "グループ:タグ名（例: XMP-dc:Title）",
  addTagValue: "値",
  addTagInvalid: "タグ名の形式が正しくありません",
  addTagNotWritable: (n: string) => `「${n}」は書き込めないタグです`,
  changedCount: (n: number) => `${n} 件の変更`,
  unsavedMark: "未保存の変更あり",

  // 保存
  save: "保存",
  saveAs: "別名で保存",
  discard: "変更を破棄",
  saveTitle: "変更を保存",
  saveAsTitle: "別名で保存",
  saveConfirm: "保存する",
  cancel: "キャンセル",
  close: "閉じる",
  colBefore: "変更前",
  colAfter: "変更後",
  optBackup: "元のファイルをバックアップとして残す（ファイル名_original）",
  optBackupNote: "すでにバックアップがある場合は、最初のバックアップがそのまま残ります",
  optPreserveTime: "ファイルの更新日時を変えない",
  saving: "保存中…",
  saved: (name: string) => `保存しました: ${name}`,
  savedNoChange: "変更はありませんでした",
  saveFailed: "保存できませんでした",
  saveWarnings: "保存しましたが、一部の変更は反映されませんでした",
  saveAsSuffix: "_edited",
  discardConfirm: (name: string) => `「${name}」の未保存の変更を破棄しますか？`,
  removeConfirm: (name: string) => `「${name}」には未保存の変更があります。破棄して一覧から外しますか？`,
  clearConfirm: (n: number) => `${n} 個のファイルに未保存の変更があります。破棄してすべて閉じますか？`,
  quitConfirm: (n: number) => `${n} 個のファイルに未保存の変更があります。破棄して終了しますか？`,
  confirmTitle: "確認",

  // よく使う項目
  quickTitle: "よく使う項目",
  fDate: "撮影日時",
  fTz: "タイムゾーン",
  fTzNone: "指定なし",
  fTitle: "タイトル",
  fDescription: "説明",
  fArtist: "作成者",
  fCopyright: "著作権",
  fKeywords: "キーワード",
  fKeywordsPlaceholder: "入力して Enter",
  fClearDate: "日時を消す",
  fDateVideoNote: "動画の日時は UTC に変換して保存されます",
  fDateImageNote: "タイムゾーンを選ぶと OffsetTime タグにも記録します",
  fDateAviNote: "AVI の日時はタイムゾーンなしで保存されます",
  fInfo: "撮影情報",
  fChanged: "変更あり",
  fRevert: "この項目を元に戻す",
  fRemoveKeyword: (k: string) => `「${k}」を外す`,
  fReadOnly: "この形式は編集できません",

  // 位置情報・地図
  fGps: "位置情報",
  fLat: "緯度",
  fLon: "経度",
  fAlt: "高度 (m)",
  fGpsInvalid: "緯度は -90〜90、経度は -180〜180 の数値で入力してください",
  fGpsShowMap: "地図で設定",
  fGpsRemove: "位置情報を削除",
  fGpsNone: "位置情報はありません",
  fGpsUnsupported: "この形式には位置情報を保存できません",
  tabTags: "タグ一覧",
  tabMap: "地図",
  mapNoPoint: "位置情報はありません",
  mapHint: "地図をクリックするかピンをドラッグすると位置を設定できます",
  mapOffline: "オフラインのため地図を表示できません（緯度・経度は右の欄に数値で入力できます）",

  // メタデータの削除
  removeMenu: "メタデータ削除",
  removeGps: "位置情報を削除",
  removeAll: "すべてのメタデータを削除…",
  removeAllConfirm: (name: string) =>
    `「${name}」のメタデータをすべて削除する変更を追加します。
画像の向きと色プロファイルは残します。保存するまでファイルは変わりません。`,
  removeAllBanner: "保存するとすべてのメタデータを削除します（画像の向きと色プロファイルは残します）。下の表の追加・変更は削除の後に書き込みます。",
  removeAllLabel: "すべてのメタデータ",

  // このアプリについて
  about: "このアプリについて",
  aboutVersion: "バージョン",
  aboutMap: "地図",
  aboutLicenseNote:
    "このアプリは以下のソフトウェアとデータを利用しています。項目を開くとライセンスの全文を表示します。",
  aboutSearch: "名前・ライセンスで絞り込み",
  aboutCount: (n: number) => `${n} 件`,

  // バイナリデータ
  binaryOpen: "中身を見る",
  binaryTitle: (tag: string) => `バイナリデータ: ${tag}`,
  binarySize: (size: string) => `${size}`,
  binaryKindImage: "画像",
  binaryKindText: "テキスト",
  binaryKindOther: "バイナリ",
  binaryViewContent: "内容",
  binaryViewHex: "16 進",
  binaryTruncated: (shown: string) => `先頭 ${shown} だけを表示しています。全体は「ファイルに保存」で取り出せます。`,
  binarySave: "ファイルに保存",
  binarySaved: (path: string) => `保存しました: ${path}`,
  binaryFailed: "読み取れませんでした",

  // よく使う項目の表示切り替え
  quickShow: "よく使う項目を表示",
  quickHide: "よく使う項目を隠す",
};

/** WebView でそのまま表示できる画像 */
export const NATIVE_IMAGE_EXT = /\.(jpe?g|png|gif|webp|bmp|avif|ico)$/i;
/** WebView で再生を試みる動画 (コーデック次第で再生できないものもある) */
export const PLAYABLE_VIDEO_EXT = /\.(mp4|m4v|mov|webm)$/i;
/** 動画として扱う拡張子 (アイコン表示用) */
export const VIDEO_EXT = /\.(mp4|m4v|mov|3gp|avi|mkv|webm|mts|m2ts)$/i;

/** ファイル選択ダイアログで使う拡張子 */
export const MEDIA_EXTENSIONS = [
  "jpg", "jpeg", "png", "tif", "tiff", "webp", "heic", "heif", "avif", "gif", "bmp",
  "dng", "cr2", "cr3", "nef", "arw", "raf", "orf", "rw2", "pef", "srw",
  "mp4", "mov", "m4v", "3gp", "avi", "mkv", "webm", "mts", "m2ts",
];
