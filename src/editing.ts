import type { FileEdits, FileMetadata, Tag, TagChange, WritableInfo } from "./types";

/** ファイル情報や計算値など、ExifTool が書き込めない (または書き込むべきでない) グループ */
const READ_ONLY_GROUPS = new Set(["ExifTool", "System", "File", "Composite"]);

export const tagKey = (t: { group: string; name: string }) => `${t.group}:${t.name}`;

/** 「すべてのメタデータを削除」を表す特別な変更 (Rust 側の is_remove_all と対) */
export const REMOVE_ALL: TagChange = { group: "", name: "all", value: null };
export const isRemoveAll = (c: TagChange) => c.group === "" && c.name === "all" && c.value === null;

/** ExifTool が "(Binary data 1234 bytes, use -b option to extract)" と表示するバイナリのタグか */
export const isBinary = (tag: Tag) => /^\(Binary data \d+ bytes/.test(tag.value);

export const fileExt = (name: string) => name.split(".").pop()?.toLowerCase() ?? "";

/** ExifTool がこの形式に書き込めるか (MKV, AVI などは読み取り専用) */
export const isFileWritable = (file: FileMetadata, info: WritableInfo | null) =>
  !!info && !file.error && info.extensions.has(fileExt(file.fileName));

export function isTagEditable(file: FileMetadata, tag: Tag, info: WritableInfo | null) {
  if (!info || !isFileWritable(file, info)) return false;
  if (READ_ONLY_GROUPS.has(tag.group)) return false;
  if (isBinary(tag)) return false;
  return info.tags.has(tag.name);
}

/** "XMP-dc:Title" や "Artist" を { group, name } に分ける */
export function parseTagInput(input: string): { group: string; name: string } | null {
  const s = input.trim().replace(/^-/, "");
  const m = s.match(/^(?:([\w-]+):)?([A-Za-z][\w-]*)$/);
  if (!m) return null;
  return { group: m[1] ?? "", name: m[2] };
}

/** 変更の一覧 (保存確認・書き込み用) */
export const changeList = (edits: FileEdits | undefined): TagChange[] =>
  edits ? Object.values(edits) : [];

export const hasEdits = (edits: FileEdits | undefined) => !!edits && Object.keys(edits).length > 0;

/** 警告付きの保存では、読み戻して反映を確認できた変更だけを確定する。 */
export function unconfirmedChanges(changes: TagChange[], file: FileMetadata): FileEdits {
  const pending: FileEdits = {};
  // 全削除を再試行する際は、その後に書き戻す指定も一緒に再実行する。
  const retryAll = changes.some(isRemoveAll);
  for (const change of changes) {
    const matches = file.tags.filter((t) => t.name === change.name && (!change.group || t.group === change.group));
    const applied = change.value === null || change.value === ""
      ? matches.length === 0
      : matches.some((t) => t.value === change.value);
    if (retryAll || !applied) pending[tagKey(change)] = change;
  }
  return pending;
}

/** 追加するタグとして入力候補に出す、よく使うタグ */
export const COMMON_NEW_TAGS = [
  "IFD0:Artist",
  "IFD0:Copyright",
  "IFD0:ImageDescription",
  "ExifIFD:DateTimeOriginal",
  "ExifIFD:UserComment",
  "XMP-dc:Title",
  "XMP-dc:Description",
  "XMP-dc:Creator",
  "XMP-dc:Rights",
  "XMP-dc:Subject",
  "IPTC:Keywords",
  "Keys:Title",
  "Keys:Description",
  "Keys:Artist",
  "UserData:Title",
];
