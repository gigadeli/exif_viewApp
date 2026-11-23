import type { FileMetadata } from "../types";
import { S, VIDEO_EXT } from "../strings";
import { ImageIcon, VideoIcon } from "./Icons";

type Props = {
  files: FileMetadata[];
  selected: string | null;
  /** 未保存の変更があるファイル */
  dirty: Set<string>;
  onSelect: (path: string) => void;
  onRemove: (path: string) => void;
};

export function FileList({ files, selected, dirty, onSelect, onRemove }: Props) {
  if (files.length === 0) {
    return <div className="file-list empty">{S.dropHint}</div>;
  }
  return (
    <ul className="file-list">
      {files.map((f) => (
        <li key={f.path} className={f.path === selected ? "selected" : undefined}>
          <button
            className="file-entry"
            onClick={() => onSelect(f.path)}
            title={f.path}
            aria-current={f.path === selected ? "true" : undefined}
          >
            <span className="file-icon">{VIDEO_EXT.test(f.fileName) ? <VideoIcon /> : <ImageIcon />}</span>
            <span className="file-name">{f.fileName}</span>
            {dirty.has(f.path) && (
              <span className="dirty-dot" title={S.unsavedMark} aria-label={S.unsavedMark}>●</span>
            )}
            {f.error && <span className="badge error" title={f.error}>!</span>}
          </button>
          <button
            className="icon-btn"
            title={S.remove}
            aria-label={`${f.fileName} を${S.remove}`}
            onClick={() => onRemove(f.path)}
          >
            ×
          </button>
        </li>
      ))}
    </ul>
  );
}
