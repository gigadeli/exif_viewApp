// Rust 側 (src-tauri/src/commands.rs) の型と対応させる

export type Tag = {
  /** -G1 のグループ名 ("IFD0", "ExifIFD", "XMP-dc", "QuickTime" など) */
  group: string;
  name: string;
  value: string;
};

export type FileMetadata = {
  path: string;
  fileName: string;
  tags: Tag[];
  error: string | null;
  warnings: string[];
  /** 読み込んだ時刻 (保存後にプレビューを読み直すため) */
  loadedAt: number;
};

/** 保存前の変更 1 件。value が null ならタグを削除 */
export type TagChange = { group: string; name: string; value: string | null };

/** "group:name" → 変更 */
export type FileEdits = Record<string, TagChange>;

export type WriteOptions = { backup: boolean; preserveFileTime: boolean };

export type WriteResult = {
  ok: boolean;
  updated: boolean;
  path: string;
  errors: string[];
  warnings: string[];
};

export type WritableInfo = { tags: Set<string>; extensions: Set<string> };
