// 位置情報 (GPS) の読み書き。
// - 画像: GPS IFD の GPSLatitude / GPSLatitudeRef など (値は絶対値 + 方角)。XMP-exif の GPS は、あれば揃える。
// - 動画: Keys:GPSCoordinates (Apple) と UserData:GPSCoordinates (©xyz) に "緯度, 経度, 高度"。
import { effectiveValue, type MediaKind } from "./fields";
import { tagKey } from "./editing";
import type { FileEdits, FileMetadata, TagChange } from "./types";

export type GpsPoint = { lat: number; lon: number; alt: number | null };

/**
 * 座標 1 つを 10 進数に。
 * "35 deg 40' 51.60\" N" / "35 deg 40' 51.60\"" / "35.681" / "-33.8688" / "35.681 S" / "35,40.86N" (XMP 形式)
 */
export function parseCoord(s: string, ref?: string): number | null {
  const t = s.trim();
  if (!t) return null;
  let sign = 1;
  const dir = (t.match(/([NSEW])\s*$/i)?.[1] ?? ref?.trim()[0] ?? "").toUpperCase();
  if (dir === "S" || dir === "W") sign = -1;
  const body = t.replace(/[NSEW]\s*$/i, "").trim();

  const dms = body.match(/^(-?\d+(?:\.\d+)?)\s*deg\s*(?:(\d+(?:\.\d+)?)'\s*)?(?:(\d+(?:\.\d+)?)")?$/);
  if (dms) {
    const v = Math.abs(+dms[1]) + (+(dms[2] ?? 0)) / 60 + (+(dms[3] ?? 0)) / 3600;
    return (dms[1].startsWith("-") ? -1 : 1) * sign * v;
  }
  const xmp = body.match(/^(\d+),(\d+(?:\.\d+)?)$/); // XMP の "度,分.分"
  if (xmp) return sign * (+xmp[1] + +xmp[2] / 60);
  if (/^-?\d+(?:\.\d+)?$/.test(body)) return sign * +body;
  return null;
}

/** "12.5 m" / "12.5 m Above Sea Level" / "3 m Below Sea Level" / "12.5" → 数値 (海面下は負) */
export function parseAltitude(s: string | undefined, ref?: string): number | null {
  if (!s) return null;
  const m = s.match(/(-?\d+(?:\.\d+)?)/);
  if (!m) return null;
  const below = /below/i.test(s) || /below|^1$/i.test(ref ?? "");
  return below ? -Math.abs(+m[1]) : +m[1];
}

const IMAGE_GPS = {
  lat: "GPS:GPSLatitude",
  latRef: "GPS:GPSLatitudeRef",
  lon: "GPS:GPSLongitude",
  lonRef: "GPS:GPSLongitudeRef",
  alt: "GPS:GPSAltitude",
  altRef: "GPS:GPSAltitudeRef",
};
const VIDEO_GPS = ["Keys:GPSCoordinates", "UserData:GPSCoordinates"];

export function readGps(file: FileMetadata, edits: FileEdits, kind: MediaKind): GpsPoint | null {
  const v = (k: string) => effectiveValue(file, edits, k);
  if (kind === "image") {
    const lat = v(IMAGE_GPS.lat);
    const lon = v(IMAGE_GPS.lon);
    if (lat !== undefined && lon !== undefined) {
      const la = parseCoord(lat, v(IMAGE_GPS.latRef));
      const lo = parseCoord(lon, v(IMAGE_GPS.lonRef));
      if (la !== null && lo !== null) {
        return { lat: la, lon: lo, alt: parseAltitude(v(IMAGE_GPS.alt), v(IMAGE_GPS.altRef)) };
      }
    }
    // GPS IFD が無く XMP にだけある場合
    const xl = v("XMP-exif:GPSLatitude");
    const xo = v("XMP-exif:GPSLongitude");
    if (xl && xo) {
      const la = parseCoord(xl);
      const lo = parseCoord(xo);
      if (la !== null && lo !== null) return { lat: la, lon: lo, alt: parseAltitude(v("XMP-exif:GPSAltitude")) };
    }
    return null;
  }
  for (const key of VIDEO_GPS) {
    const s = v(key);
    if (!s) continue;
    const [a, b, c] = s.split(",").map((x) => x.trim());
    const la = parseCoord(a ?? "");
    const lo = parseCoord(b ?? "");
    if (la !== null && lo !== null) return { lat: la, lon: lo, alt: parseAltitude(c) };
  }
  return null;
}

/** 位置情報に関係するタグ (変更ありの判定・元に戻す・削除用) */
export function gpsKeys(file: FileMetadata, kind: MediaKind, edits: FileEdits = {}): string[] {
  const existing = [...file.tags, ...Object.values(edits)]
    .filter((t) => t.group !== "Composite" && (t.group === "GPS" || /^GPS/.test(t.name)))
    .map(tagKey);
  const base = kind === "image" ? Object.values(IMAGE_GPS) : VIDEO_GPS;
  return [...new Set([...base, ...existing])];
}

const round = (n: number) => String(Math.round(n * 1e7) / 1e7);

export function isValidPoint(p: GpsPoint) {
  return Number.isFinite(p.lat) && Number.isFinite(p.lon) && Math.abs(p.lat) <= 90 && Math.abs(p.lon) <= 180;
}

/** 位置を設定 (null なら位置情報を削除) したときの変更 */
export function setGps(
  file: FileMetadata,
  edits: FileEdits,
  kind: MediaKind,
  p: GpsPoint | null,
): TagChange[] {
  const present = (k: string) => file.tags.some((t) => tagKey(t) === k) || (k in edits && edits[k].value !== null);
  const change = (key: string, value: string | null): TagChange => {
    const i = key.indexOf(":");
    return { group: key.slice(0, i), name: key.slice(i + 1), value };
  };

  if (p === null) {
    // ファイルにある (または追加予定の) GPS 関連タグをすべて削除
    return gpsKeys(file, kind, edits)
      .filter((k) => effectiveValue(file, edits, k) !== undefined)
      .map((k) => change(k, null));
  }
  if (!isValidPoint(p)) return [];

  if (kind === "image") {
    const changes = [
      change(IMAGE_GPS.lat, round(Math.abs(p.lat))),
      change(IMAGE_GPS.latRef, p.lat < 0 ? "S" : "N"),
      change(IMAGE_GPS.lon, round(Math.abs(p.lon))),
      change(IMAGE_GPS.lonRef, p.lon < 0 ? "W" : "E"),
    ];
    if (p.alt !== null) {
      changes.push(change(IMAGE_GPS.alt, String(Math.abs(p.alt))));
      changes.push(change(IMAGE_GPS.altRef, p.alt < 0 ? "Below Sea Level" : "Above Sea Level"));
    } else {
      for (const k of [IMAGE_GPS.alt, IMAGE_GPS.altRef]) if (present(k)) changes.push(change(k, null));
    }
    // XMP にもあれば揃える (符号付きの 10 進数で書ける)
    if (present("XMP-exif:GPSLatitude")) changes.push(change("XMP-exif:GPSLatitude", round(p.lat)));
    if (present("XMP-exif:GPSLongitude")) changes.push(change("XMP-exif:GPSLongitude", round(p.lon)));
    return changes;
  }

  const value = [round(p.lat), round(p.lon), ...(p.alt !== null ? [String(p.alt)] : [])].join(", ");
  return VIDEO_GPS.map((k) => change(k, value));
}

/** 表示用 "35.681000, 139.767000" */
export const formatPoint = (p: GpsPoint) => `${p.lat.toFixed(6)}, ${p.lon.toFixed(6)}`;
