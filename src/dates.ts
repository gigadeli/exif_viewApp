// 日時とタイムゾーンの変換。
// - EXIF の日時は「撮影地のローカル時刻」で、タイムゾーンは別タグ (OffsetTimeOriginal) に入る。
// - QuickTime (MP4/MOV) の CreateDate などは UTC で入る。Apple の Keys:CreationDate はローカル時刻 + タイムゾーン。

/** 日付と時刻 (タイムゾーンなし) */
export type LocalDateTime = {
  year: number;
  month: number;
  day: number;
  hour: number;
  minute: number;
  second: number;
};

/** "+09:00" / "-05:30" / "" (指定なし) */
export type TzOffset = string;

const pad = (n: number, w = 2) => String(n).padStart(w, "0");

/**
 * ExifTool の日時文字列を読む。
 * "2026:11:01 10:20:30", "2026:11:01 10:20:30.123+09:00", "2026:11:01 10:20:30Z" など。
 * 日付だけ ("2026:11:01") も受け付ける。0000:00:00 は「無し」として null。
 */
export function parseExifDate(s: string): { dt: LocalDateTime; tz: TzOffset } | null {
  const m = s
    .trim()
    .match(/^(\d{4})[:-](\d{2})[:-](\d{2})(?:[ T](\d{2}):(\d{2})(?::(\d{2}))?(?:\.\d+)?)?\s*(Z|[+-]\d{2}:?\d{2})?$/);
  if (!m) return null;
  const [, y, mo, d, h = "0", mi = "0", sec = "0", tz] = m;
  if (Number(y) === 0 || Number(mo) === 0 || Number(d) === 0) return null;
  return {
    dt: { year: +y, month: +mo, day: +d, hour: +h, minute: +mi, second: +sec },
    tz: normalizeTz(tz ?? ""),
  };
}

/** "Z" → "+00:00"、"+0900" → "+09:00" */
export function normalizeTz(tz: string): TzOffset {
  if (!tz) return "";
  if (tz === "Z") return "+00:00";
  const m = tz.match(/^([+-])(\d{2}):?(\d{2})$/);
  return m ? `${m[1]}${m[2]}:${m[3]}` : "";
}

export const tzToMinutes = (tz: TzOffset): number => {
  const m = tz.match(/^([+-])(\d{2}):(\d{2})$/);
  if (!m) return 0;
  const v = Number(m[2]) * 60 + Number(m[3]);
  return m[1] === "-" ? -v : v;
};

export const minutesToTz = (min: number): TzOffset => {
  const sign = min < 0 ? "-" : "+";
  const a = Math.abs(min);
  return `${sign}${pad(Math.floor(a / 60))}:${pad(a % 60)}`;
};

/** ExifTool に書く形式 "YYYY:MM:DD HH:MM:SS" (tz を付けると末尾に +09:00) */
export const formatExifDate = (dt: LocalDateTime, tz: TzOffset = "") =>
  `${pad(dt.year, 4)}:${pad(dt.month)}:${pad(dt.day)} ${pad(dt.hour)}:${pad(dt.minute)}:${pad(dt.second)}${tz}`;

/** <input type="datetime-local"> の値 "YYYY-MM-DDTHH:MM[:SS]" */
export const toInputValue = (dt: LocalDateTime) =>
  `${pad(dt.year, 4)}-${pad(dt.month)}-${pad(dt.day)}T${pad(dt.hour)}:${pad(dt.minute)}:${pad(dt.second)}`;

export function fromInputValue(v: string): LocalDateTime | null {
  const m = v.match(/^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?/);
  if (!m) return null;
  return { year: +m[1], month: +m[2], day: +m[3], hour: +m[4], minute: +m[5], second: +(m[6] ?? 0) };
}

const toEpoch = (dt: LocalDateTime) =>
  Date.UTC(dt.year, dt.month - 1, dt.day, dt.hour, dt.minute, dt.second);

const fromEpoch = (ms: number): LocalDateTime => {
  const d = new Date(ms);
  return {
    year: d.getUTCFullYear(),
    month: d.getUTCMonth() + 1,
    day: d.getUTCDate(),
    hour: d.getUTCHours(),
    minute: d.getUTCMinutes(),
    second: d.getUTCSeconds(),
  };
};

/** ローカル時刻 + タイムゾーン → UTC */
export const localToUtc = (dt: LocalDateTime, tz: TzOffset) =>
  fromEpoch(toEpoch(dt) - tzToMinutes(tz) * 60_000);

/** UTC → 指定タイムゾーンのローカル時刻 */
export const utcToLocal = (dt: LocalDateTime, tz: TzOffset) =>
  fromEpoch(toEpoch(dt) + tzToMinutes(tz) * 60_000);

/** この PC のタイムゾーン (その日時での夏時間も考慮) */
export const pcTzAt = (utc: LocalDateTime | null): TzOffset => {
  const d = utc ? new Date(toEpoch(utc)) : new Date();
  return minutesToTz(-d.getTimezoneOffset());
};

/** タイムゾーンの選択肢 (UTC-12:00 〜 UTC+14:00、よく使われる 30 分・45 分単位を含む) */
export const TZ_OPTIONS: TzOffset[] = (() => {
  const mins = new Set<number>();
  for (let h = -12; h <= 14; h++) mins.add(h * 60);
  for (const m of [-570, -210, 210, 270, 330, 345, 390, 525, 570, 630, 765]) mins.add(m);
  return [...mins].sort((a, b) => a - b).map(minutesToTz);
})();
