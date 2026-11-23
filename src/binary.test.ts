import { describe, expect, it } from "vitest";
import { decodeBase64, formatBytes, hexDump } from "./binary";

describe("binary helpers", () => {
  it("formats sizes", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(3 * 1024 * 1024)).toBe("3.00 MB");
  });

  it("decodes base64 to bytes", () => {
    expect(Array.from(decodeBase64("/9j/"))).toEqual([0xff, 0xd8, 0xff]);
  });

  it("dumps hex with offsets and ASCII, padding the last row", () => {
    const bytes = new Uint8Array([...Array.from("ABCDEFGHIJKLMNOP", (c) => c.charCodeAt(0)), 0x00, 0xff]);
    const lines = hexDump(bytes).split("\n");
    expect(lines[0]).toBe("00000000  41 42 43 44 45 46 47 48 49 4a 4b 4c 4d 4e 4f 50  ABCDEFGHIJKLMNOP");
    expect(lines[1]).toBe(`00000010  00 ff${" ".repeat(42)}  ..`);
  });
});
