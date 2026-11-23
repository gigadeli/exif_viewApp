import { useEffect, useMemo, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { readBinary, saveBinary, type BinaryData } from "../api";
import { decodeBase64, formatBytes, hexDump } from "../binary";
import { S } from "../strings";
import type { FileMetadata, Tag } from "../types";

type Props = { file: FileMetadata; tag: Tag; onClose: () => void };

const EXT: Record<string, string> = {
  "image/jpeg": "jpg",
  "image/png": "png",
  "image/gif": "gif",
  "image/webp": "webp",
  "image/bmp": "bmp",
};

/** バイナリのタグの中身を表示する (画像ならそのまま、テキストなら文字、それ以外は 16 進) */
export function BinaryDialog({ file, tag, onClose }: Props) {
  const [data, setData] = useState<BinaryData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<"content" | "hex">("content");
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);
  const label = `${tag.group}:${tag.name}`;

  useEffect(() => {
    readBinary(file.path, tag.group, tag.name)
      .then((d) => {
        setData(d);
        if (d.kind === "other") setView("hex");
      })
      .catch((e) => setError(String(e)));
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [file.path, tag.group, tag.name, onClose]);

  const bytes = useMemo(() => (data ? decodeBase64(data.base64) : null), [data]);
  const text = useMemo(() => (bytes && data?.kind === "text" ? new TextDecoder().decode(bytes) : ""), [bytes, data]);

  const onSave = async () => {
    if (!data) return;
    const ext = data.mime ? EXT[data.mime] : data.kind === "text" ? "txt" : "bin";
    const stem = file.fileName.replace(/\.[^.]+$/, "");
    const dest = await save({ defaultPath: `${stem}_${tag.name}.${ext}` });
    if (!dest) return;
    try {
      await saveBinary(file.path, tag.group, tag.name, dest);
      setMessage({ text: S.binarySaved(dest), error: false });
    } catch (e) {
      setMessage({ text: String(e), error: true });
    }
  };

  const kindLabel =
    data?.kind === "image" ? S.binaryKindImage : data?.kind === "text" ? S.binaryKindText : S.binaryKindOther;

  return (
    <div className="modal-backdrop">
      <div className="modal binary" role="dialog" aria-modal="true" aria-labelledby="binary-title">
        <h2 id="binary-title">{S.binaryTitle(label)}</h2>
        {error && <div className="notice error">{S.binaryFailed}: {error}</div>}
        {!data && !error && <p className="muted">{S.loading}</p>}
        {data && bytes && (
          <>
            <div className="binary-meta">
              <span>{kindLabel}{data.mime ? ` (${data.mime})` : ""}</span>
              <span className="muted">{formatBytes(data.size)}</span>
              <span className="spacer" />
              {data.kind !== "other" && (
                <div className="tabs small-tabs" role="tablist">
                  <button role="tab" aria-selected={view === "content"} className={view === "content" ? "active" : ""} onClick={() => setView("content")}>
                    {S.binaryViewContent}
                  </button>
                  <button role="tab" aria-selected={view === "hex"} className={view === "hex" ? "active" : ""} onClick={() => setView("hex")}>
                    {S.binaryViewHex}
                  </button>
                </div>
              )}
            </div>
            <div className="binary-body">
              {view === "content" && data.kind === "image" && (
                <img src={`data:${data.mime};base64,${data.base64}`} alt={label} />
              )}
              {view === "content" && data.kind === "text" && <pre className="binary-pre">{text}</pre>}
              {view === "hex" && <pre className="binary-pre hex">{hexDump(bytes)}</pre>}
            </div>
            {data.truncated && <small className="muted">{S.binaryTruncated(formatBytes(bytes.length))}</small>}
          </>
        )}
        <div className="modal-actions">
          {message && (
            <span className={`small-text ${message.error ? "status-error" : "muted"}`} title={message.text}>
              {message.text}
            </span>
          )}
          <span className="spacer" />
          <button onClick={onSave} disabled={!data}>
            {S.binarySave}
          </button>
          <button className="primary" onClick={onClose} autoFocus>
            {S.close}
          </button>
        </div>
      </div>
    </div>
  );
}
