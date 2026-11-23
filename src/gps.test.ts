import { describe, expect, it } from "vitest";
import { gpsKeys, parseAltitude, parseCoord, readGps, setGps } from "./gps";
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
const none: FileEdits = {};
const close = (a: number | null | undefined, b: number) => expect(a!).toBeCloseTo(b, 6);
const map = (changes: { group: string; name: string; value: string | null }[]) =>
  Object.fromEntries(changes.map((c) => [`${c.group}:${c.name}`, c.value]));

describe("parseCoord", () => {
  it("reads ExifTool DMS output", () => {
    close(parseCoord(`35 deg 40' 51.60" N`), 35.681);
    close(parseCoord(`33 deg 52' 7.68" S`), -33.8688);
    close(parseCoord(`151 deg 12' 33.48"`, "West"), -151.2093);
  });
  it("reads decimal and XMP forms", () => {
    close(parseCoord("35.681"), 35.681);
    close(parseCoord("-33.8688"), -33.8688);
    close(parseCoord("35.681 S"), -35.681);
    close(parseCoord("35,40.86N"), 35.681);
  });
  it("rejects garbage", () => {
    expect(parseCoord("")).toBeNull();
    expect(parseCoord("abc")).toBeNull();
  });
  it("reads altitude with reference", () => {
    expect(parseAltitude("12.5 m Above Sea Level")).toBe(12.5);
    expect(parseAltitude("3 m Below Sea Level")).toBe(-3);
    expect(parseAltitude("3 m", "Below Sea Level")).toBe(-3);
    expect(parseAltitude(undefined)).toBeNull();
  });
});

describe("image GPS", () => {
  const jpg = file([
    ["GPS:GPSVersionID", "2.3.0.0"],
    ["GPS:GPSLatitudeRef", "North"],
    ["GPS:GPSLatitude", `35 deg 40' 51.60"`],
    ["GPS:GPSLongitudeRef", "East"],
    ["GPS:GPSLongitude", `139 deg 46' 1.20"`],
    ["XMP-exif:GPSLatitude", `35 deg 40' 51.60" N`],
    ["Composite:GPSPosition", "..."],
  ]);

  it("reads position from the GPS group", () => {
    const p = readGps(jpg, none, "image")!;
    close(p.lat, 35.681);
    close(p.lon, 139.767);
    expect(p.alt).toBeNull();
  });

  it("writes absolute values with refs, and syncs existing XMP", () => {
    expect(map(setGps(jpg, none, "image", { lat: -33.8688, lon: -151.2093, alt: -2 }))).toEqual({
      "GPS:GPSLatitude": "33.8688",
      "GPS:GPSLatitudeRef": "S",
      "GPS:GPSLongitude": "151.2093",
      "GPS:GPSLongitudeRef": "W",
      "GPS:GPSAltitude": "2",
      "GPS:GPSAltitudeRef": "Below Sea Level",
      "XMP-exif:GPSLatitude": "-33.8688",
    });
  });

  it("reads back pending edits written by setGps", () => {
    const edits: FileEdits = {};
    for (const c of setGps(jpg, none, "image", { lat: -33.8688, lon: -151.2093, alt: null })) {
      edits[`${c.group}:${c.name}`] = c;
    }
    const p = readGps(jpg, edits, "image")!;
    close(p.lat, -33.8688);
    close(p.lon, -151.2093);
  });

  it("removing deletes every existing GPS tag except Composite", () => {
    const keys = Object.keys(map(setGps(jpg, none, "image", null))).sort();
    expect(keys).toEqual(
      ["GPS:GPSLatitude", "GPS:GPSLatitudeRef", "GPS:GPSLongitude", "GPS:GPSLongitudeRef", "GPS:GPSVersionID", "XMP-exif:GPSLatitude"].sort(),
    );
    expect(gpsKeys(jpg, "image")).not.toContain("Composite:GPSPosition");
  });

  it("ignores out-of-range input", () => {
    expect(setGps(jpg, none, "image", { lat: 91, lon: 0, alt: null })).toEqual([]);
  });

  it("removes incomplete coordinates and pending GPS additions only once", () => {
    const partial = file([["GPS:GPSLatitude", "35"]]);
    const added = { group: "XMP-exif", name: "GPSAltitude", value: "10" };
    const edits = { "XMP-exif:GPSAltitude": added };
    expect(readGps(partial, edits, "image")).toBeNull();
    const removal = setGps(partial, edits, "image", null);
    expect(map(removal)).toEqual({ "GPS:GPSLatitude": null, "XMP-exif:GPSAltitude": null });
    const deleted = Object.fromEntries(removal.map((c) => [`${c.group}:${c.name}`, c]));
    expect(setGps(partial, deleted, "image", null)).toEqual([]);
  });
});

describe("video GPS", () => {
  const mp4 = file([["UserData:GPSCoordinates", `35 deg 40' 51.60" N, 139 deg 46' 1.20" E, 10 m Above Sea Level`]]);

  it("reads GPSCoordinates", () => {
    const p = readGps(mp4, none, "video")!;
    close(p.lat, 35.681);
    close(p.lon, 139.767);
    expect(p.alt).toBe(10);
  });

  it("writes both Keys and UserData", () => {
    expect(map(setGps(mp4, none, "video", { lat: 35.681, lon: 139.767, alt: null }))).toEqual({
      "Keys:GPSCoordinates": "35.681, 139.767",
      "UserData:GPSCoordinates": "35.681, 139.767",
    });
  });

  it("removing deletes only tags that exist", () => {
    expect(map(setGps(mp4, none, "video", null))).toEqual({ "UserData:GPSCoordinates": null });
  });
});
