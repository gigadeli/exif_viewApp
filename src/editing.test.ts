import { expect, it } from "vitest";
import { canWriteTag, isFileWritable, isTagEditable, nativeTagSuggestions, REMOVE_ALL, unconfirmedChanges } from "./editing";
import type { FileMetadata, WritableInfo } from "./types";

const title = { group: "XMP-dc", name: "Title", value: "保存済み" };
const file: FileMetadata = {
  path: "copy.jpg", fileName: "copy.jpg", loadedAt: 0, error: null, warnings: [], tags: [title],
};

it("retains only unconfirmed edits after a partial write", () => {
  const failed = { group: "IFD0", name: "Artist", value: "再試行" };
  const removed = { group: "XMP-dc", name: "Subject", value: null };
  expect(unconfirmedChanges([title, failed, removed], file)).toEqual({ "IFD0:Artist": failed });
  const failedDeletion = { ...title, value: null };
  expect(unconfirmedChanges([failedDeletion], file)).toEqual({ "XMP-dc:Title": failedDeletion });
});

it("keeps the tags to restore when retrying a remove-all operation", () => {
  expect(unconfirmedChanges([REMOVE_ALL, title], file)).toEqual({ ":all": REMOVE_ALL, "XMP-dc:Title": title });
});

const info: WritableInfo = {
  tags: new Set(["Title", "Artist"]),
  extensions: new Set(["jpg", "mp4"]),
  native: {
    fileTypes: new Set(["AVI", "MKV", "WEBM"]),
    tags: new Map([
      ["RIFF", new Set(["Title", "Artist"])],
      ["Info", new Set(["Title", "DateTimeOriginal"])],
      ["Matroska", new Set(["Title", "Artist"])],
    ]),
  },
};
const media = (fileName: string, type: string): FileMetadata => ({
  path: fileName,
  fileName,
  error: null,
  warnings: [],
  loadedAt: 0,
  tags: [{ group: "File", name: "FileType", value: type }],
});

it("treats AVI / MKV / WebM as writable by the native writer", () => {
  const avi = media("a.avi", "AVI");
  const mkv = media("a.mkv", "MKV");
  expect(isFileWritable(avi, info)).toBe(true);
  expect(isFileWritable(media("a.webm", "WEBM"), info)).toBe(true);
  expect(isFileWritable(media("a.flv", "FLV"), info)).toBe(false);
  expect(isTagEditable(avi, { group: "RIFF", name: "Title", value: "" }, info)).toBe(true);
  expect(isTagEditable(avi, { group: "RIFF", name: "VideoCodec", value: "" }, info)).toBe(false);
  expect(isTagEditable(mkv, { group: "Track1", name: "Title", value: "" }, info)).toBe(false);
  expect(canWriteTag(mkv, info, "Info", "DateTimeOriginal")).toBe(true);
  expect(canWriteTag(mkv, info, "", "Title")).toBe(false);
  expect(nativeTagSuggestions(avi, info)).toEqual(["RIFF:Title", "RIFF:Artist"]);
  expect(nativeTagSuggestions(media("a.jpg", "JPEG"), info)).toEqual([]);
});
