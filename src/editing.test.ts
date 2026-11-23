import { expect, it } from "vitest";
import { REMOVE_ALL, unconfirmedChanges } from "./editing";
import type { FileMetadata } from "./types";

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
