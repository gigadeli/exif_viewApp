import { describe, expect, it } from "vitest";
import {
  formatExifDate,
  fromInputValue,
  localToUtc,
  normalizeTz,
  parseExifDate,
  TZ_OPTIONS,
  utcToLocal,
} from "./dates";
import { dateClearable, mediaKind, readDate, readTextField, setDate, setTextField } from "./fields";
import type { FileEdits, FileMetadata } from "./types";

const file = (tags: [string, string][]): FileMetadata => ({
  path: "x",
  fileName: "x",
  error: null,
  warnings: [],
  loadedAt: 0,
  tags: tags.map(([key, value]) => {
    const i = key.indexOf(":");
    return { group: key.slice(0, i), name: key.slice(i + 1), value };
  }),
});

const dt = (s: string) => parseExifDate(s)!.dt;

describe("dates", () => {
  it("parses ExifTool date strings", () => {
    expect(parseExifDate("2026:11:01 10:20:30")).toEqual({
      dt: { year: 2026, month: 11, day: 1, hour: 10, minute: 20, second: 30 },
      tz: "",
    });
    expect(parseExifDate("2026:11:01 10:20:30.123+09:00")?.tz).toBe("+09:00");
    expect(parseExifDate("2026:11:01 10:20:30Z")?.tz).toBe("+00:00");
    expect(parseExifDate("2026:11:01")?.dt.hour).toBe(0);
    expect(parseExifDate("0000:00:00 00:00:00")).toBeNull();
    expect(parseExifDate("not a date")).toBeNull();
  });

  it("normalizes offsets", () => {
    expect(normalizeTz("+0900")).toBe("+09:00");
    expect(normalizeTz("-05:30")).toBe("-05:30");
    expect(normalizeTz("")).toBe("");
  });

  it("converts local <-> UTC across day and year boundaries", () => {
    expect(formatExifDate(localToUtc(dt("2026:01:01 05:00:00"), "+09:00"))).toBe("2025:12:31 20:00:00");
    expect(formatExifDate(localToUtc(dt("2026:03:01 23:30:00"), "-05:30"))).toBe("2026:03:02 05:00:00");
    expect(formatExifDate(utcToLocal(dt("2025:12:31 20:00:00"), "+09:00"))).toBe("2026:01:01 05:00:00");
    // うるう日
    expect(formatExifDate(localToUtc(dt("2028:03:01 01:00:00"), "+09:00"))).toBe("2028:02:29 16:00:00");
  });

  it("reads datetime-local input values with or without seconds", () => {
    expect(fromInputValue("2026-11-01T10:20")).toEqual(dt("2026:11:01 10:20:00"));
    expect(fromInputValue("2026-11-01T10:20:30")).toEqual(dt("2026:11:01 10:20:30"));
  });

  it("offers sorted timezone options including +09:00 and +05:45", () => {
    expect(TZ_OPTIONS).toContain("+09:00");
    expect(TZ_OPTIONS).toContain("+05:45");
    expect(TZ_OPTIONS[0]).toBe("-12:00");
    expect(TZ_OPTIONS[TZ_OPTIONS.length - 1]).toBe("+14:00");
  });
});

describe("image fields", () => {
  const jpg = file([
    ["File:MIMEType", "image/jpeg"],
    ["IFD0:Artist", "古い作者"],
    ["IPTC:By-line", "IPTC作者"],
    ["XMP-dc:Title", "タイトル"],
    ["ExifIFD:DateTimeOriginal", "2026:11:01 10:20:30"],
    ["ExifIFD:OffsetTimeOriginal", "+09:00"],
    ["XMP-xmp:CreateDate", "2026:11:01 10:20:30+09:00"],
  ]);
  const none: FileEdits = {};

  it("detects kind from MIME type", () => {
    expect(mediaKind(jpg)).toBe("image");
  });

  it("reads text fields in priority order", () => {
    expect(readTextField(jpg, none, "image", "artist")).toBe("古い作者");
    expect(readTextField(jpg, none, "image", "title")).toBe("タイトル");
    expect(readTextField(jpg, none, "image", "copyright")).toBe("");
  });

  it("writes main tags and also updates tags that already exist", () => {
    const keys = setTextField(jpg, none, "image", "artist", "新しい作者").map((c) => `${c.group}:${c.name}`);
    expect(keys.sort()).toEqual(["IFD0:Artist", "IPTC:By-line", "XMP-dc:Creator"].sort());
  });

  it("does not create IPTC tags that were not there", () => {
    const keys = setTextField(jpg, none, "image", "copyright", "(c)").map((c) => `${c.group}:${c.name}`);
    expect(keys.sort()).toEqual(["IFD0:Copyright", "XMP-dc:Rights"]);
  });

  it("clearing a field deletes only existing tags", () => {
    const changes = setTextField(jpg, none, "image", "title", "");
    expect(changes).toEqual([{ group: "XMP-dc", name: "Title", value: null }]);
  });

  it("reads date with timezone from OffsetTimeOriginal", () => {
    expect(readDate(jpg, none, "image")).toEqual({ dt: dt("2026:11:01 10:20:30"), tz: "+09:00" });
  });

  it("writes EXIF local time, offsets, and existing XMP dates with timezone", () => {
    const changes = setDate(jpg, none, "image", dt("2027:01:02 03:04:05"), "+09:00");
    const map = Object.fromEntries(changes.map((c) => [`${c.group}:${c.name}`, c.value]));
    expect(map).toEqual({
      "ExifIFD:DateTimeOriginal": "2027:01:02 03:04:05",
      "ExifIFD:CreateDate": "2027:01:02 03:04:05",
      "ExifIFD:OffsetTimeOriginal": "+09:00",
      "ExifIFD:OffsetTimeDigitized": "+09:00",
      "XMP-xmp:CreateDate": "2027:01:02 03:04:05+09:00",
    });
  });

  it("removing the timezone deletes existing offset tags", () => {
    const changes = setDate(jpg, none, "image", dt("2027:01:02 03:04:05"), "");
    const map = Object.fromEntries(changes.map((c) => [`${c.group}:${c.name}`, c.value]));
    expect(map["ExifIFD:OffsetTimeOriginal"]).toBeNull();
    expect("ExifIFD:OffsetTimeDigitized" in map).toBe(false); // 元から無いので削除もしない
  });

  it("reflects pending edits when reading", () => {
    const edits: FileEdits = { "IFD0:Artist": { group: "IFD0", name: "Artist", value: "編集後" } };
    expect(readTextField(jpg, edits, "image", "artist")).toBe("編集後");
  });
});

describe("video fields", () => {
  const mp4 = file([
    ["File:MIMEType", "video/mp4"],
    ["QuickTime:CreateDate", "2026:11:01 01:20:30"],
    ["Track1:TrackCreateDate", "2026:11:01 01:20:30"],
    ["Track1:MediaCreateDate", "2026:11:01 01:20:30"],
    ["Track2:TrackCreateDate", "2026:11:01 01:20:30"],
    ["Keys:Title", "Apple のタイトル"],
  ]);
  const none: FileEdits = {};

  it("detects kind and reads Keys:Title when ItemList is absent", () => {
    expect(mediaKind(mp4)).toBe("video");
    expect(readTextField(mp4, none, "video", "title")).toBe("Apple のタイトル");
  });

  it("writes ItemList and keeps existing Keys tag in sync", () => {
    const keys = setTextField(mp4, none, "video", "title", "新").map((c) => `${c.group}:${c.name}`);
    expect(keys.sort()).toEqual(["ItemList:Title", "Keys:Title"]);
  });

  it("writes QuickTime dates in UTC and CreationDate in local time", () => {
    const changes = setDate(mp4, none, "video", dt("2026:11:01 10:20:30"), "+09:00");
    const map = Object.fromEntries(changes.map((c) => [`${c.group}:${c.name}`, c.value]));
    expect(map).toEqual({
      "QuickTime:CreateDate": "2026:11:01 01:20:30",
      "Track1:TrackCreateDate": "2026:11:01 01:20:30",
      "Track1:MediaCreateDate": "2026:11:01 01:20:30",
      "Track2:TrackCreateDate": "2026:11:01 01:20:30",
      "Keys:CreationDate": "2026:11:01 10:20:30+09:00",
    });
  });

  it("prefers Keys:CreationDate with its timezone when reading", () => {
    const withApple = file([
      ["File:MIMEType", "video/quicktime"],
      ["QuickTime:CreateDate", "2026:11:01 01:20:30"],
      ["Keys:CreationDate", "2026:11:01 10:20:30+09:00"],
    ]);
    expect(readDate(withApple, none, "video")).toEqual({ dt: dt("2026:11:01 10:20:30"), tz: "+09:00" });
  });

  it("treats zero dates as empty", () => {
    const zero = file([["File:MIMEType", "video/mp4"], ["QuickTime:CreateDate", "0000:00:00 00:00:00"]]);
    expect(readDate(zero, none, "video").dt).toBeNull();
  });
});

describe("AVI / MKV / WebM fields", () => {
  const avi = file([
    ["File:FileType", "AVI"],
    ["File:MIMEType", "video/x-msvideo"],
    ["RIFF:Title", "AVI のタイトル"],
    ["RIFF:DateTimeOriginal", "2024:05:01 10:20:30"],
  ]);
  const mkv = file([
    ["File:FileType", "MKV"],
    ["File:MIMEType", "video/x-matroska"],
    ["Info:Title", "情報のタイトル"],
    ["Matroska:Title", "タグのタイトル"],
    ["Info:DateTimeOriginal", "2024:05:01 01:20:30Z"],
  ]);
  const none: FileEdits = {};

  it("detects the kind from FileType", () => {
    expect(mediaKind(avi)).toBe("avi");
    expect(mediaKind(mkv)).toBe("matroska");
    expect(mediaKind(file([["File:FileType", "WEBM"]]))).toBe("matroska");
  });

  it("reads and writes AVI INFO tags", () => {
    expect(readTextField(avi, none, "avi", "title")).toBe("AVI のタイトル");
    const keys = setTextField(avi, none, "avi", "artist", "作者").map((c) => `${c.group}:${c.name}=${c.value}`);
    expect(keys).toEqual(["RIFF:Artist=作者"]);
  });

  it("writes the AVI date to DateCreated without a timezone", () => {
    expect(readDate(avi, none, "avi")).toEqual({ dt: dt("2024:05:01 10:20:30"), tz: "" });
    const changes = setDate(avi, none, "avi", dt("2025:01:02 03:04:05"), "+09:00");
    expect(changes).toEqual([{ group: "RIFF", name: "DateCreated", value: "2025:01:02 03:04:05" }]);
  });

  it("writes the Matroska title to Info and keeps the Title tag in sync", () => {
    expect(readTextField(mkv, none, "matroska", "title")).toBe("情報のタイトル");
    const keys = setTextField(mkv, none, "matroska", "title", "新").map((c) => `${c.group}:${c.name}`);
    expect(keys.sort()).toEqual(["Info:Title", "Matroska:Title"]);
  });

  it("stores the Matroska date in UTC", () => {
    const changes = setDate(mkv, none, "matroska", dt("2026:11:01 10:20:30"), "+09:00");
    expect(changes).toEqual([{ group: "Info", name: "DateTimeOriginal", value: "2026:11:01 01:20:30Z" }]);
    const read = readDate(mkv, none, "matroska");
    expect(localToUtc(read.dt!, read.tz)).toEqual(dt("2024:05:01 01:20:30"));
  });
});

describe("AVI date clearing", () => {
  const none: FileEdits = {};
  it("cannot clear a date that only exists in the read-only IDIT", () => {
    const idit = file([["File:FileType", "AVI"], ["RIFF:DateTimeOriginal", "2024:05:01 10:20:30"]]);
    expect(readDate(idit, none, "avi").dt).not.toBeNull();
    expect(dateClearable(idit, none, "avi")).toBe(false);
    const created = file([["File:FileType", "AVI"], ["RIFF:DateCreated", "2024:05:01 10:20:30"]]);
    expect(dateClearable(created, none, "avi")).toBe(true);
    expect(setDate(created, none, "avi", null, "")).toEqual([{ group: "RIFF", name: "DateCreated", value: null }]);
  });
});
