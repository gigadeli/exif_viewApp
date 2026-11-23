import { useEffect, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { exportMetadata, type ExportFormat } from "../api";
import { S } from "../strings";
import type { FileMetadata } from "../types";
import { ChevronDown } from "./Icons";

type Props = {
  files: FileMetadata[];
  current: FileMetadata | null;
  onResult: (message: string, isError: boolean) => void;
};

/** "写真.jpg" → "写真" */
const stem = (name: string) => name.replace(/\.[^.]+$/, "");

export function ExportMenu({ files, current, onResult }: Props) {
  const [open, setOpen] = useState(false);
  // 保存ダイアログ表示中・書き出し中は二重に実行させない
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);

  const run = async (targets: FileMetadata[], format: ExportFormat) => {
    setOpen(false);
    setBusy(true);
    try {
      await runExport(targets, format);
    } finally {
      setBusy(false);
    }
  };

  const runExport = async (targets: FileMetadata[], format: ExportFormat) => {
    const base = targets.length === 1 ? stem(targets[0].fileName) : S.exportDefaultName;
    const dest = await save({
      defaultPath: `${base}.${format}`,
      filters: [{ name: format.toUpperCase(), extensions: [format] }],
    });
    if (!dest) return;
    try {
      await exportMetadata(targets.map((f) => f.path), format, dest);
      onResult(S.exportDone(dest), false);
    } catch (e) {
      onResult(`${S.exportFailed}: ${e}`, true);
    }
  };

  const items: { label: string; targets: FileMetadata[]; format: ExportFormat }[] = [
    { label: S.exportJsonCurrent, targets: current ? [current] : [], format: "json" },
    { label: S.exportCsvCurrent, targets: current ? [current] : [], format: "csv" },
    { label: S.exportJsonAll, targets: files, format: "json" },
    { label: S.exportCsvAll, targets: files, format: "csv" },
  ];

  return (
    <div className="menu" ref={ref}>
      <button onClick={() => setOpen((o) => !o)} disabled={files.length === 0 || busy}>
        {S.exportMenu} <ChevronDown />
      </button>
      {open && (
        <ul className="menu-list" role="menu">
          {items.map((it, i) => (
            <li key={it.label} role="none">
              {i === 2 && <hr />}
              <button
                role="menuitem"
                disabled={it.targets.length === 0}
                onClick={() => run(it.targets, it.format)}
              >
                {it.label}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
