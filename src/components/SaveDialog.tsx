import { useEffect, useState } from "react";
import { tagKey } from "../editing";
import { S } from "../strings";
import type { FileMetadata, TagChange, WriteOptions, WriteResult } from "../types";

type Props = {
  file: FileMetadata;
  changes: TagChange[];
  saveAs: boolean;
  busy: boolean;
  onConfirm: (options: WriteOptions) => void;
  onCancel: () => void;
};

const loadBool = (key: string, fallback: boolean) => {
  const v = localStorage.getItem(key);
  return v === null ? fallback : v === "true";
};

/** 保存前に変更内容を確認するダイアログ */
export function SaveDialog({ file, changes, saveAs, busy, onConfirm, onCancel }: Props) {
  const [backup, setBackup] = useState(() => loadBool("opt.backup", true));
  const [preserveFileTime, setPreserveFileTime] = useState(() => loadBool("opt.preserveFileTime", false));

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !busy && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onCancel]);

  const before = new Map(file.tags.map((t) => [tagKey(t), t.value]));

  const confirm = () => {
    localStorage.setItem("opt.backup", String(backup));
    localStorage.setItem("opt.preserveFileTime", String(preserveFileTime));
    onConfirm({ backup, preserveFileTime });
  };

  return (
    <div className="modal-backdrop">
      <div className="modal" role="dialog" aria-modal="true" aria-labelledby="save-title">
        <h2 id="save-title">{saveAs ? S.saveAsTitle : S.saveTitle}</h2>
        <p className="muted">{file.fileName} — {S.changedCount(changes.length)}</p>
        <div className="change-scroll">
          <table className="change-table">
            <thead>
              <tr>
                <th>{S.colTag}</th>
                <th>{S.colBefore}</th>
                <th>{S.colAfter}</th>
              </tr>
            </thead>
            <tbody>
              {changes.map((c) => {
                const key = tagKey(c);
                return (
                  <tr key={key}>
                    <td>{key === ":all" ? <strong>{S.removeAllLabel}</strong> : key.replace(/^:/, "")}</td>
                    <td className="muted">{before.get(key) ?? "—"}</td>
                    <td>{c.value === null ? <span className="status-error">{S.deleted}</span> : c.value}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
        {!saveAs && (
          <label className="check">
            <input type="checkbox" checked={backup} onChange={(e) => setBackup(e.target.checked)} />
            <span>
              {S.optBackup}
              <br />
              <small className="muted">{S.optBackupNote}</small>
            </span>
          </label>
        )}
        <label className="check">
          <input type="checkbox" checked={preserveFileTime} onChange={(e) => setPreserveFileTime(e.target.checked)} />
          <span>{S.optPreserveTime}</span>
        </label>
        <div className="modal-actions">
          <button onClick={onCancel} disabled={busy}>
            {S.cancel}
          </button>
          <button className="primary" onClick={confirm} disabled={busy} autoFocus>
            {busy ? S.saving : saveAs ? S.saveAs : S.saveConfirm}
          </button>
        </div>
      </div>
    </div>
  );
}

/** 保存に失敗したとき・一部が反映されなかったときの結果表示 */
export function SaveResultDialog({ result, onClose }: { result: WriteResult; onClose: () => void }) {
  return (
    <div className="modal-backdrop">
      <div className="modal" role="alertdialog" aria-modal="true" aria-labelledby="result-title">
        <h2 id="result-title">{result.ok ? S.saveWarnings : S.saveFailed}</h2>
        <ul className="messages">
          {result.errors.map((m, i) => (
            <li key={`e${i}`} className="status-error">{m}</li>
          ))}
          {result.warnings.map((m, i) => (
            <li key={`w${i}`}>{m}</li>
          ))}
        </ul>
        <div className="modal-actions">
          <button className="primary" onClick={onClose} autoFocus>
            {S.close}
          </button>
        </div>
      </div>
    </div>
  );
}
