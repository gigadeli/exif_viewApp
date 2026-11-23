// 「よく使う項目」(論理的な項目) と、実際に読み書きするタグの対応。
// 画像と動画でタグの置き場所が違うので、種類ごとに対応表を持つ。
import {
  formatExifDate,
  localToUtc,
  parseExifDate,
  pcTzAt,
  utcToLocal,
  type LocalDateTime,
  type TzOffset,
} from "./dates";
import { tagKey } from "./editing";
import type { FileEdits, FileMetadata, TagChange } from "./types";

/** image / video は ExifTool で書く形式、avi / matroska は自作ライターで書く形式 (タグの置き場所が違う) */
export type MediaKind = "image" | "video" | "avi" | "matroska";
export type TextFieldId = "title" | "description" | "artist" | "copyright" | "keywords";

type TextFieldMap = {
  /** 表示する値を探す順 */
  read: string[];
  /** 常に書き込むタグ */
  write: string[];
  /** ファイルに既にある場合だけ、同じ値に揃えるタグ (古い値が残って食い違うのを防ぐ) */
  ifPresent: string[];
};

export const TEXT_FIELDS: Record<MediaKind, Record<TextFieldId, TextFieldMap>> = {
  image: {
    title: { read: ["XMP-dc:Title", "IPTC:ObjectName"], write: ["XMP-dc:Title"], ifPresent: ["IPTC:ObjectName"] },
    description: {
      read: ["XMP-dc:Description", "IFD0:ImageDescription", "IPTC:Caption-Abstract"],
      write: ["XMP-dc:Description", "IFD0:ImageDescription"],
      ifPresent: ["IPTC:Caption-Abstract"],
    },
    artist: {
      read: ["XMP-dc:Creator", "IFD0:Artist", "IPTC:By-line"],
      write: ["IFD0:Artist", "XMP-dc:Creator"],
      ifPresent: ["IPTC:By-line"],
    },
    copyright: {
      read: ["XMP-dc:Rights", "IFD0:Copyright", "IPTC:CopyrightNotice"],
      write: ["IFD0:Copyright", "XMP-dc:Rights"],
      ifPresent: ["IPTC:CopyrightNotice"],
    },
    keywords: { read: ["XMP-dc:Subject", "IPTC:Keywords"], write: ["XMP-dc:Subject"], ifPresent: ["IPTC:Keywords"] },
  },
  video: {
    title: {
      read: ["ItemList:Title", "Keys:Title", "UserData:Title", "XMP-dc:Title"],
      write: ["ItemList:Title"],
      ifPresent: ["Keys:Title", "UserData:Title", "XMP-dc:Title"],
    },
    description: {
      read: ["ItemList:Description", "Keys:Description", "UserData:Description", "ItemList:Comment"],
      write: ["ItemList:Description"],
      ifPresent: ["Keys:Description", "UserData:Description"],
    },
    artist: {
      read: ["ItemList:Artist", "Keys:Artist", "Keys:Author", "UserData:Artist", "UserData:Author"],
      write: ["ItemList:Artist"],
      ifPresent: ["Keys:Artist", "Keys:Author", "UserData:Artist", "UserData:Author"],
    },
    copyright: {
      read: ["ItemList:Copyright", "Keys:Copyright", "UserData:Copyright"],
      write: ["ItemList:Copyright"],
      ifPresent: ["Keys:Copyright", "UserData:Copyright"],
    },
    keywords: { read: ["XMP-dc:Subject", "Keys:Keywords"], write: ["XMP-dc:Subject"], ifPresent: ["Keys:Keywords"] },
  },
  // AVI: RIFF の INFO チャンク
  avi: {
    title: { read: ["RIFF:Title"], write: ["RIFF:Title"], ifPresent: [] },
    description: { read: ["RIFF:Comment", "RIFF:Subject"], write: ["RIFF:Comment"], ifPresent: [] },
    artist: { read: ["RIFF:Artist"], write: ["RIFF:Artist"], ifPresent: [] },
    copyright: { read: ["RIFF:Copyright"], write: ["RIFF:Copyright"], ifPresent: [] },
    keywords: { read: ["RIFF:Keywords"], write: ["RIFF:Keywords"], ifPresent: [] },
  },
  // MKV / WebM: タイトルは Segment の Info、他は全体向けの SimpleTag
  matroska: {
    title: { read: ["Info:Title", "Matroska:Title"], write: ["Info:Title"], ifPresent: ["Matroska:Title"] },
    description: {
      read: ["Matroska:Description", "Matroska:Comment"],
      write: ["Matroska:Description"],
      ifPresent: [],
    },
    artist: { read: ["Matroska:Artist"], write: ["Matroska:Artist"], ifPresent: [] },
    copyright: { read: ["Matroska:Copyright"], write: ["Matroska:Copyright"], ifPresent: [] },
    keywords: { read: ["Matroska:Keywords"], write: ["Matroska:Keywords"], ifPresent: [] },
  },
};

const MATROSKA_TYPES = new Set(["MKV", "MKA", "MKS", "WEBM"]);

export function mediaKind(file: FileMetadata): MediaKind {
  const type = file.tags.find((t) => t.group === "File" && t.name === "FileType")?.value ?? "";
  if (type === "AVI") return "avi";
  if (MATROSKA_TYPES.has(type)) return "matroska";
  const mime = file.tags.find((t) => t.name === "MIMEType")?.value ?? "";
  return mime.startsWith("video/") ? "video" : "image";
}

/** 位置情報を書き込める種類か (AVI / MKV / WebM は対応していない) */
export const supportsGps = (kind: MediaKind) => kind === "image" || kind === "video";

/** タイムゾーンを保存できない種類 (AVI の日時はローカル時刻の文字列だけ) */
export const dateHasTz = (kind: MediaKind) => kind !== "avi";

const splitKey = (key: string) => {
  const i = key.indexOf(":");
  return { group: key.slice(0, i), name: key.slice(i + 1) };
};

/** 変更を反映した後のタグの値。無ければ undefined (削除予定も undefined) */
export function effectiveValue(file: FileMetadata, edits: FileEdits, key: string): string | undefined {
  if (key in edits) return edits[key].value ?? undefined;
  return file.tags.find((t) => tagKey(t) === key)?.value;
}

/** ファイルにある (または追加予定の) タグか */
const isPresent = (file: FileMetadata, edits: FileEdits, key: string) =>
  effectiveValue(file, edits, key) !== undefined || file.tags.some((t) => tagKey(t) === key);

export function readTextField(file: FileMetadata, edits: FileEdits, kind: MediaKind, id: TextFieldId) {
  for (const key of TEXT_FIELDS[kind][id].read) {
    const v = effectiveValue(file, edits, key);
    if (v !== undefined && v !== "") return v;
  }
  return "";
}

/** 項目に関係するタグ (変更ありの判定・元に戻す用) */
export function textFieldKeys(kind: MediaKind, id: TextFieldId) {
  const m = TEXT_FIELDS[kind][id];
  return [...new Set([...m.read, ...m.write, ...m.ifPresent])];
}

/** 項目に値を入れたときの変更。空文字はタグの削除 */
export function setTextField(
  file: FileMetadata,
  edits: FileEdits,
  kind: MediaKind,
  id: TextFieldId,
  value: string,
): TagChange[] {
  const m = TEXT_FIELDS[kind][id];
  const v = value === "" ? null : value;
  const keys = [...m.write, ...m.ifPresent.filter((k) => isPresent(file, edits, k))];
  // 読み取り候補にしか無いタグ (動画の ItemList:Comment など) は残っていても表示が食い違うので、あれば揃える
  for (const k of m.read) if (!keys.includes(k) && isPresent(file, edits, k)) keys.push(k);
  return keys
    .filter((k) => v !== null || isPresent(file, edits, k)) // 無いタグを「削除」する変更は作らない
    .map((k) => ({ ...splitKey(k), value: v }));
}

// ---------- 撮影日時 ----------

export type DateField = { dt: LocalDateTime | null; tz: TzOffset };

const IMAGE_DATE_READ = [
  "ExifIFD:DateTimeOriginal",
  "XMP-exif:DateTimeOriginal",
  "XMP-photoshop:DateCreated",
  "ExifIFD:CreateDate",
  "XMP-xmp:CreateDate",
];
const IMAGE_DATE_IF_PRESENT = ["XMP-exif:DateTimeOriginal", "XMP-xmp:CreateDate", "XMP-photoshop:DateCreated"];
const IMAGE_TZ_TAGS = ["ExifIFD:OffsetTimeOriginal", "ExifIFD:OffsetTimeDigitized"];
/** 動画のトラックごとの日時 (Track1, Track2 … のどのグループにもある) */
const VIDEO_TRACK_DATE_NAMES = ["TrackCreateDate", "MediaCreateDate"];

const AVI_DATE_READ = ["RIFF:DateCreated", "RIFF:DateTimeOriginal"];

/**
 * 表示中の日時を消せるか。AVI の DateTimeOriginal (hdrl の IDIT) は書き込めないので、
 * それしか無い場合は消せない (消しても表示が変わらない)
 */
export function dateClearable(file: FileMetadata, edits: FileEdits, kind: MediaKind): boolean {
  if (!readDate(file, edits, kind).dt) return false;
  if (kind !== "avi") return true;
  return !!parseExifDate(effectiveValue(file, edits, "RIFF:DateCreated") ?? "");
}
const MKV_DATE = "Info:DateTimeOriginal";
const MKV_DATE_TAG = "Matroska:DateTimeOriginal";

/** UTC の日時文字列を、PC のタイムゾーンでの表示にする */
function utcField(v: string | undefined): DateField | null {
  const pu = v && parseExifDate(v);
  if (!pu) return null;
  const tz = pcTzAt(pu.dt);
  return { dt: utcToLocal(pu.dt, tz), tz };
}

export function readDate(file: FileMetadata, edits: FileEdits, kind: MediaKind): DateField {
  if (kind === "avi") {
    for (const key of AVI_DATE_READ) {
      const p = parseExifDate(effectiveValue(file, edits, key) ?? "");
      if (p) return { dt: p.dt, tz: "" };
    }
    return { dt: null, tz: "" };
  }
  if (kind === "matroska") {
    const fromInfo = utcField(effectiveValue(file, edits, MKV_DATE));
    if (fromInfo) return fromInfo;
    // SimpleTag の DATE_RECORDED (タイムゾーンが書かれていれば、それを使う)
    const p = parseExifDate(effectiveValue(file, edits, MKV_DATE_TAG) ?? "");
    if (p) return p.tz ? { dt: p.dt, tz: p.tz } : (utcField(effectiveValue(file, edits, MKV_DATE_TAG)) as DateField);
    return { dt: null, tz: pcTzAt(null) };
  }
  if (kind === "image") {
    for (const key of IMAGE_DATE_READ) {
      const v = effectiveValue(file, edits, key);
      const p = v && parseExifDate(v);
      if (p) {
        const offset = effectiveValue(file, edits, "ExifIFD:OffsetTimeOriginal") ?? "";
        return { dt: p.dt, tz: p.tz || (parseExifDate(`2000:01:01 00:00:00${offset}`)?.tz ?? "") };
      }
    }
    return { dt: null, tz: "" };
  }
  // 動画: Apple の CreationDate (ローカル + tz) を優先、無ければ UTC の CreateDate を PC のタイムゾーンで表示
  const apple = effectiveValue(file, edits, "Keys:CreationDate");
  const pa = apple && parseExifDate(apple);
  if (pa && pa.tz) return { dt: pa.dt, tz: pa.tz };
  const utcStr = effectiveValue(file, edits, "QuickTime:CreateDate");
  const pu = utcStr && parseExifDate(utcStr);
  if (pu) {
    const tz = pcTzAt(pu.dt);
    return { dt: utcToLocal(pu.dt, tz), tz };
  }
  return { dt: null, tz: pcTzAt(null) };
}

export function dateFieldKeys(file: FileMetadata, kind: MediaKind): string[] {
  if (kind === "image") return [...IMAGE_DATE_READ, ...IMAGE_TZ_TAGS];
  if (kind === "avi") return AVI_DATE_READ;
  if (kind === "matroska") return [MKV_DATE, MKV_DATE_TAG];
  const trackKeys = file.tags.filter((t) => VIDEO_TRACK_DATE_NAMES.includes(t.name)).map(tagKey);
  return ["QuickTime:CreateDate", "Keys:CreationDate", ...trackKeys];
}

/** 撮影日時を設定したときの変更。dt が null なら日時を削除 */
export function setDate(
  file: FileMetadata,
  edits: FileEdits,
  kind: MediaKind,
  dt: LocalDateTime | null,
  tz: TzOffset,
): TagChange[] {
  const changes: TagChange[] = [];
  const set = (key: string, value: string | null) => {
    if (value === null && !isPresent(file, edits, key)) return;
    changes.push({ ...splitKey(key), value });
  };

  if (kind === "avi") {
    // DateTimeOriginal (hdrl の IDIT) は書き込めないので、INFO の DateCreated に書く
    set("RIFF:DateCreated", dt && formatExifDate(dt));
    return changes;
  }
  if (kind === "matroska") {
    const utc = dt && formatExifDate(localToUtc(dt, tz || "+00:00"));
    set(MKV_DATE, utc && `${utc}Z`);
    if (isPresent(file, edits, MKV_DATE_TAG)) set(MKV_DATE_TAG, utc);
    return changes;
  }

  if (kind === "image") {
    const local = dt && formatExifDate(dt);
    set("ExifIFD:DateTimeOriginal", local);
    set("ExifIFD:CreateDate", local);
    for (const k of IMAGE_TZ_TAGS) set(k, dt && tz ? tz : null);
    // XMP の日時はタイムゾーン付きで書ける
    for (const k of IMAGE_DATE_IF_PRESENT) if (isPresent(file, edits, k)) set(k, dt && formatExifDate(dt, tz));
    return changes;
  }

  // 動画: QuickTime の日時は UTC で書く
  const utc = dt && formatExifDate(localToUtc(dt, tz || "+00:00"));
  set("QuickTime:CreateDate", utc);
  for (const t of file.tags.filter((t) => VIDEO_TRACK_DATE_NAMES.includes(t.name))) set(tagKey(t), utc);
  set("Keys:CreationDate", dt && formatExifDate(dt, tz || "+00:00"));
  return changes;
}

// ---------- 参照用の情報 ----------

/** カメラ・撮影情報として表示するタグ (読み取り専用) */
export const INFO_TAGS: { label: string; names: string[] }[] = [
  { label: "カメラ", names: ["Make", "Model"] },
  { label: "レンズ", names: ["LensModel", "Lens"] },
  { label: "露出", names: ["ExposureTime", "FNumber", "ISO"] },
  { label: "焦点距離", names: ["FocalLength"] },
  { label: "サイズ", names: ["ImageSize"] },
  { label: "長さ", names: ["Duration"] },
];

/** 名前で最初に見つかったタグの値 (グループは問わない、Composite を優先) */
export function findByName(file: FileMetadata, name: string): string | undefined {
  const all = file.tags.filter((t) => t.name === name);
  return (all.find((t) => t.group === "Composite") ?? all[0])?.value;
}
