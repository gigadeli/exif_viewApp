import { useCallback, useEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import {
  exiftoolVersion,
  readMetadata,
  saveAs as saveAsCommand,
  startupPaths,
  writableInfo,
  writeMetadata,
} from "./api";
import { AboutDialog } from "./components/AboutDialog";
import { BinaryDialog } from "./components/BinaryDialog";
import { ExportMenu } from "./components/ExportMenu";
import { MapView } from "./components/MapView";
import { RemoveMenu } from "./components/RemoveMenu";
import { FileList } from "./components/FileList";
import { Preview } from "./components/Preview";
import { QuickEdit } from "./components/QuickEdit";
import { SaveDialog, SaveResultDialog } from "./components/SaveDialog";
import { TagTable } from "./components/TagTable";
import { changeList, fileExt, hasEdits, isFileWritable, isRemoveAll, REMOVE_ALL, tagKey, unconfirmedChanges } from "./editing";
import { mediaKind, supportsGps } from "./fields";
import { readGps, setGps } from "./gps";
import { MEDIA_EXTENSIONS, S } from "./strings";
import type {
  FileEdits,
  Tag,
  FileMetadata,
  TagChange,
  WritableInfo,
  WriteOptions,
  WriteResult,
} from "./types";
import "./App.css";

/** 保存ダイアログの状態。別名保存のときは保存先も持つ */
type SavePlan = { path: string; dest: string | null };

const confirmDiscard = (message: string) => ask(message, { title: S.confirmTitle, kind: "warning" });

export default function App() {
  const [files, setFiles] = useState<FileMetadata[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [edits, setEdits] = useState<Record<string, FileEdits>>({});
  const [writable, setWritable] = useState<WritableInfo | null>(null);
  const [loading, setLoading] = useState(false);
  const [dragOver, setDragOver] = useState(false);
  const [version, setVersion] = useState<string | null>(null);
  const [fatal, setFatal] = useState<string | null>(null);
  const [status, setStatus] = useState<{ text: string; error: boolean } | null>(null);
  const [savePlan, setSavePlan] = useState<SavePlan | null>(null);
  const [view, setView] = useState<"tags" | "map">("tags");
  const [showAbout, setShowAbout] = useState(false);
  const [binaryTag, setBinaryTag] = useState<Tag | null>(null);
  const [showQuick, setShowQuick] = useState(() => localStorage.getItem("showQuick") !== "false");
  const toggleQuick = () =>
    setShowQuick((v) => {
      localStorage.setItem("showQuick", String(!v));
      return !v;
    });
  const [saving, setSaving] = useState(false);
  const [saveResult, setSaveResult] = useState<WriteResult | null>(null);
  const [showPreview, setShowPreview] = useState(
    () => localStorage.getItem("showPreview") !== "false",
  );
  const togglePreview = () =>
    setShowPreview((v) => {
      localStorage.setItem("showPreview", String(!v));
      return !v;
    });

  const current = files.find((f) => f.path === selected) ?? null;
  const currentEdits = (current && edits[current.path]) || {};
  const dirtyPaths = new Set(Object.keys(edits).filter((p) => hasEdits(edits[p])));
  const gpsRemovals = current ? setGps(current, currentEdits, mediaKind(current), null) : [];
  const modalOpen = !!savePlan || saving || !!saveResult || showAbout;

  // 起動時に ExifTool を準備 (初回は exe から展開するので少し時間がかかる)
  useEffect(() => {
    exiftoolVersion()
      .then((v) => {
        setVersion(v);
        return writableInfo().then(setWritable);
      })
      .catch((e) => setFatal(String(e)));
  }, []);

  const addPaths = useCallback(async (paths: string[]) => {
    if (paths.length === 0) return;
    setLoading(true);
    try {
      const result = await readMetadata(paths);
      setFiles((prev) => {
        const map = new Map(prev.map((f) => [f.path, f]));
        for (const f of result) map.set(f.path, f); // 同じファイルは読み直した結果で置き換え
        return [...map.values()];
      });
      setSelected(result[result.length - 1].path);
      setFatal(null);
      return result;
    } catch (e) {
      setFatal(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  // コマンドライン引数で渡されたファイルを開く (StrictMode で 2 回走らないようにする)
  const startupDone = useRef(false);
  useEffect(() => {
    if (startupDone.current) return;
    startupDone.current = true;
    startupPaths().then(addPaths).catch(() => {});
  }, [addPaths]);

  // ドラッグ&ドロップ
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      const p = event.payload;
      if (p.type === "enter" || p.type === "over") setDragOver(true);
      else if (p.type === "leave") setDragOver(false);
      else if (p.type === "drop") {
        setDragOver(false);
        addPaths(p.paths);
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [addPaths]);

  // 未保存の変更があるまま閉じようとしたら確認する
  const dirtyCount = useRef(0);
  dirtyCount.current = dirtyPaths.size;
  const asking = useRef(false);
  useEffect(() => {
    const unlisten = getCurrentWindow().onCloseRequested(async (event) => {
      if (dirtyCount.current === 0) return;
      // 確認中にもう一度 × が押されても、ダイアログを重ねて出さない
      if (asking.current) {
        event.preventDefault();
        return;
      }
      asking.current = true;
      try {
        if (!(await confirmDiscard(S.quitConfirm(dirtyCount.current)))) event.preventDefault();
      } finally {
        asking.current = false;
      }
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const onOpen = async () => {
    const picked = await open({
      multiple: true,
      directory: false,
      filters: [
        { name: S.dialogFilterMedia, extensions: MEDIA_EXTENSIONS },
        { name: S.dialogFilterAll, extensions: ["*"] },
      ],
    });
    if (picked) addPaths(picked);
  };

  const dropEdits = (paths: string[]) =>
    setEdits((prev) => {
      const next = { ...prev };
      for (const p of paths) delete next[p];
      return next;
    });

  const onRemove = async (path: string) => {
    const file = files.find((f) => f.path === path);
    if (file && hasEdits(edits[path]) && !(await confirmDiscard(S.removeConfirm(file.fileName)))) return;
    const next = files.filter((f) => f.path !== path);
    setFiles(next);
    dropEdits([path]);
    if (selected === path) setSelected(next[0]?.path ?? null);
  };

  const onClearAll = async () => {
    if (dirtyPaths.size > 0 && !(await confirmDiscard(S.clearConfirm(dirtyPaths.size)))) return;
    setFiles([]);
    setEdits({});
    setSelected(null);
  };

  // ---- 編集 ----
  /** 変更をまとめて反映する。元の値に戻っただけの変更は「変更なし」として取り除く */
  const onApply = (changes: TagChange[]) => {
    if (!current || changes.length === 0) return;
    const file = current;
    const original = new Map(file.tags.map((t) => [tagKey(t), t.value]));
    setEdits((prev) => {
      const next = { ...prev[file.path] };
      for (const c of changes) {
        const key = tagKey(c);
        const before = original.get(key);
        if (isRemoveAll(c)) {
          next[key] = c;
        } else if ((c.value !== null && c.value === before) || (c.value === null && before === undefined)) {
          delete next[key];
        } else {
          next[key] = c;
        }
      }
      return { ...prev, [file.path]: next };
    });
  };
  const onChange = (change: TagChange) => onApply([change]);

  const onRevertKeys = (keys: string[]) => {
    if (!current) return;
    const path = current.path;
    setEdits((prev) => {
      const next = { ...prev[path] };
      for (const k of keys) delete next[k];
      return { ...prev, [path]: next };
    });
  };
  const onRevert = (key: string) => onRevertKeys([key]);

  const onDiscard = async () => {
    if (!current || !(await confirmDiscard(S.discardConfirm(current.fileName)))) return;
    dropEdits([current.path]);
  };

  // ---- 保存 ----
  const canSave = !!current && hasEdits(currentEdits) && isFileWritable(current, writable);

  const onSave = () => {
    if (!modalOpen && canSave && current) setSavePlan({ path: current.path, dest: null });
  };

  const onSaveAs = async () => {
    if (!current) return;
    const ext = fileExt(current.fileName);
    const stem = current.fileName.replace(/\.[^.]+$/, "");
    const dest = await save({
      defaultPath: `${stem}${S.saveAsSuffix}.${ext}`,
      filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
    });
    if (dest) setSavePlan({ path: current.path, dest });
  };

  const runSave = async (plan: SavePlan, options: WriteOptions) => {
    const file = files.find((f) => f.path === plan.path);
    if (!file) return;
    setSaving(true);
    try {
      const changes = changeList(edits[plan.path]);
      const result = plan.dest
        ? await saveAsCommand(plan.path, plan.dest, changes, options)
        : await writeMetadata(plan.path, changes, options);
      if (result.ok) {
        const refreshed = (await addPaths([result.path]))?.[0];
        if (!result.warnings.length || (refreshed && !refreshed.error)) {
          const pending = result.warnings.length && refreshed ? unconfirmedChanges(changes, refreshed) : {};
          setEdits((prev) => {
            const next = { ...prev, [plan.path]: { ...prev[plan.path] } };
            for (const change of changes) {
              const key = tagKey(change);
              if (next[plan.path][key] === change) delete next[plan.path][key];
            }
            next[result.path] = { ...pending, ...next[result.path] };
            return next;
          });
        }
        setStatus({
          text: result.updated || plan.dest ? S.saved(result.path) : S.savedNoChange,
          error: false,
        });
      }
      setSavePlan(null);
      if (!result.ok || result.warnings.length > 0) setSaveResult(result);
    } catch (e) {
      setSavePlan(null);
      setSaveResult({ ok: false, updated: false, path: plan.path, errors: [String(e)], warnings: [] });
    } finally {
      setSaving(false);
    }
  };

  // Ctrl+S で保存
  const onSaveRef = useRef(onSave);
  onSaveRef.current = onSave;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.key.toLowerCase() === "s") {
        e.preventDefault();
        if (modalOpen || e.isComposing || e.repeat) return;
        const active = document.activeElement;
        if (active instanceof HTMLInputElement || active instanceof HTMLTextAreaElement) {
          // blur が確定した編集を含むレンダーの後で保存可否を判定する。
          flushSync(() => active.blur());
        }
        onSaveRef.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [modalOpen]);

  const planFile = savePlan && files.find((f) => f.path === savePlan.path);
  const closeAbout = useCallback(() => setShowAbout(false), []);
  const closeBinary = useCallback(() => setBinaryTag(null), []);

  return (
    <div className="app">
      <header className="toolbar" inert={modalOpen}>
        <button className="primary" onClick={onOpen} disabled={loading}>
          {S.open}
        </button>
        <button onClick={onClearAll} disabled={files.length === 0}>
          {S.clear}
        </button>
        <span className="divider" />
        <button onClick={onSave} disabled={!canSave} title="Ctrl+S">
          {S.save}
        </button>
        <button onClick={onSaveAs} disabled={!current || !isFileWritable(current, writable)}>
          {S.saveAs}
        </button>
        <button onClick={onDiscard} disabled={!current || !hasEdits(currentEdits)}>
          {S.discard}
        </button>
        <span className="divider" />
        <RemoveMenu
          disabled={!current || !isFileWritable(current, writable)}
          hasGps={gpsRemovals.length > 0}
          onRemoveGps={() => onApply(gpsRemovals)}
          onRemoveAll={async () => {
            if (current && (await ask(S.removeAllConfirm(current.fileName), { title: S.confirmTitle, kind: "warning" }))) {
              onApply([REMOVE_ALL]);
            }
          }}
        />
        <ExportMenu
          files={files}
          current={current}
          onResult={(text, error) => setStatus({ text, error })}
        />
        {loading && <span className="muted">{S.loading}</span>}
        <span className="spacer" />
        <button onClick={toggleQuick} aria-pressed={showQuick}>
          {showQuick ? S.quickHide : S.quickShow}
        </button>
        <button onClick={() => setShowAbout(true)}>{S.about}</button>
      </header>

      <main className="body" inert={modalOpen}>
        <aside className="sidebar">
          <FileList
            files={files}
            selected={selected}
            dirty={dirtyPaths}
            onSelect={setSelected}
            onRemove={onRemove}
          />
        </aside>
        <section className="content">
          {fatal && <div className="notice error">{fatal}</div>}
          {current ? (
            <>
              <div className="file-header" title={current.path}>
                <h1>{current.fileName}</h1>
                <span className="muted path">{current.path}</span>
                <span className="spacer" />
                <button className="small" onClick={togglePreview}>
                  {showPreview ? S.previewHide : S.previewShow}
                </button>
              </div>
              {/* 兄弟要素の key は重複させない (重複すると古い要素が残る) */}
              {showPreview && (
                <Preview key={`preview:${current.path}:${current.loadedAt}`} file={current} />
              )}
              <div className="tabs" role="tablist">
                <button role="tab" aria-selected={view === "tags"} className={view === "tags" ? "active" : ""} onClick={() => setView("tags")}>
                  {S.tabTags}
                </button>
                <button role="tab" aria-selected={view === "map"} className={view === "map" ? "active" : ""} onClick={() => setView("map")}>
                  {S.tabMap}
                </button>
              </div>
              {view === "tags" ? (
                <TagTable
                  key={`tags:${current.path}`}
                  file={current}
                  edits={currentEdits}
                  writable={writable}
                  onChange={onChange}
                  onRevert={onRevert}
                  onShowBinary={setBinaryTag}
                />
              ) : (
                <MapView
                  key={`map:${current.path}`}
                  point={readGps(current, currentEdits, mediaKind(current))}
                  editable={isFileWritable(current, writable) && supportsGps(mediaKind(current))}
                  onPick={(lat, lon) => {
                    const kind = mediaKind(current);
                    const alt = readGps(current, currentEdits, kind)?.alt ?? null;
                    onApply(setGps(current, currentEdits, kind, { lat, lon, alt }));
                  }}
                />
              )}
            </>
          ) : (
            <div className="placeholder">{files.length === 0 ? S.dropHint : S.noSelection}</div>
          )}
        </section>
        {current && !current.error && showQuick && (
          <aside className="quick-panel">
            <QuickEdit
              key={`quick:${current.path}`}
              file={current}
              edits={currentEdits}
              writable={writable}
              onApply={onApply}
              onRevertKeys={onRevertKeys}
              onShowMap={() => setView("map")}
              onHide={toggleQuick}
            />
          </aside>
        )}
      </main>

      <footer className="statusbar">
        <span>{S.fileCount(files.length)}</span>
        {status && (
          <span className={status.error ? "status-error" : undefined} title={status.text}>
            {status.text}
          </span>
        )}
        <span className="spacer" />
        <span>{version ? S.exiftool(version) : fatal ? "" : S.starting}</span>
      </footer>

      {dragOver && <div className="drop-overlay">{S.dropOverlay}</div>}

      {savePlan && planFile && (
        <SaveDialog
          file={planFile}
          changes={changeList(edits[savePlan.path])}
          saveAs={!!savePlan.dest}
          busy={saving}
          onConfirm={(options) => runSave(savePlan, options)}
          onCancel={() => setSavePlan(null)}
        />
      )}
      {saveResult && <SaveResultDialog result={saveResult} onClose={() => setSaveResult(null)} />}
      {showAbout && <AboutDialog exiftoolVersion={version} onClose={closeAbout} />}
      {binaryTag && current && <BinaryDialog file={current} tag={binaryTag} onClose={closeBinary} />}
    </div>
  );
}
