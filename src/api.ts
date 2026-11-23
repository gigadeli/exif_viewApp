import { invoke } from "@tauri-apps/api/core";
import type { FileMetadata, TagChange, WritableInfo, WriteOptions, WriteResult } from "./types";

export const readMetadata = async (paths: string[]) => {
  const list = await invoke<Omit<FileMetadata, "loadedAt">[]>("read_metadata", { paths });
  const now = Date.now();
  return list.map((f) => ({ ...f, loadedAt: now }));
};

export const exiftoolVersion = () => invoke<string>("exiftool_version");

export const startupPaths = () => invoke<string[]>("startup_paths");

export const getEmbeddedPreview = (path: string) =>
  invoke<string | null>("get_embedded_preview", { path });

export type ExportFormat = "json" | "csv";

export const exportMetadata = (paths: string[], format: ExportFormat, dest: string) =>
  invoke<void>("export_metadata", { paths, format, dest });

export const writableInfo = async (): Promise<WritableInfo> => {
  const r = await invoke<{ tags: string[]; extensions: string[] }>("writable_info");
  return { tags: new Set(r.tags), extensions: new Set(r.extensions) };
};

export const writeMetadata = (path: string, changes: TagChange[], options: WriteOptions) =>
  invoke<WriteResult>("write_metadata", { path, changes, options });

export const saveAs = (path: string, dest: string, changes: TagChange[], options: WriteOptions) =>
  invoke<WriteResult>("save_as", { path, dest, changes, options });

export type BinaryData = {
  size: number;
  kind: "image" | "text" | "other";
  mime: string | null;
  base64: string;
  truncated: boolean;
};

export const readBinary = (path: string, group: string, name: string) =>
  invoke<BinaryData>("read_binary", { path, group, name });

export const saveBinary = (path: string, group: string, name: string, dest: string) =>
  invoke<number>("save_binary", { path, group, name, dest });
