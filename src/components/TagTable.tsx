import { useMemo, useRef, useState } from "react";
import {
  canWriteTag,
  COMMON_NEW_TAGS,
  isBinary,
  isFileWritable,
  isNativeFormat,
  isRemoveAll,
  isTagEditable,
  nativeTagSuggestions,
  parseTagInput,
  tagKey,
} from "../editing";
import type { FileEdits, FileMetadata, Tag, TagChange, WritableInfo } from "../types";
import { S } from "../strings";

type Props = {
  file: FileMetadata;
  edits: FileEdits;
  writable: WritableInfo | null;
  onChange: (change: TagChange) => void;
  onRevert: (key: string) => void;
  onShowBinary: (tag: Tag) => void;
};

/** 表に出す 1 行。既存タグか、新しく追加するタグ */
type Row = { tag: Tag; isNew: boolean };

export function TagTable({ file, edits, writable, onChange, onRevert, onShowBinary }: Props) {
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  // Enter / Esc で編集を閉じた直後の blur で、もう一度確定しないようにする
  const closing = useRef(false);
  const fileWritable = isFileWritable(file, writable);

  // 既存タグ + 追加予定のタグ (既存に無い変更) をグループごとにまとめる
  const groups = useMemo(() => {
    const q = query.trim().toLowerCase();
    const match = (t: Tag, value: string) =>
      !q ||
      t.name.toLowerCase().includes(q) ||
      t.group.toLowerCase().includes(q) ||
      value.toLowerCase().includes(q);

    const map = new Map<string, Row[]>();
    const existing = new Set<string>();
    for (const t of file.tags) {
      const key = tagKey(t);
      existing.add(key);
      const shown = edits[key] ? (edits[key].value ?? "") : t.value;
      if (!match(t, shown)) continue;
      const list = map.get(t.group) ?? [];
      list.push({ tag: t, isNew: false });
      map.set(t.group, list);
    }
    const added: Row[] = Object.entries(edits)
      .filter(([key, c]) => !existing.has(key) && !isRemoveAll(c))
      .map(([, c]) => ({ tag: { group: c.group, name: c.name, value: "" }, isNew: true }))
      .filter((r) => match(r.tag, edits[tagKey(r.tag)].value ?? ""));
    const result = [...map.entries()];
    if (added.length) result.unshift([S.newGroup, added]);
    return result;
  }, [file.tags, edits, query]);

  const toggle = (g: string) =>
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(g)) next.delete(g);
      else next.add(g);
      return next;
    });

  const startEdit = (row: Row) => {
    const key = tagKey(row.tag);
    closing.current = false;
    setEditing(key);
    setDraft(edits[key]?.value ?? row.tag.value);
  };

  const commit = (row: Row) => {
    if (closing.current) return;
    closing.current = true;
    const key = tagKey(row.tag);
    setEditing(null);
    if (!row.isNew && draft === row.tag.value) {
      onRevert(key); // 元の値に戻した = 変更なし
      return;
    }
    onChange({ group: row.tag.group, name: row.tag.name, value: draft });
  };

  return (
    <div className="tag-panel">
      <div className="tag-toolbar">
        <input
          type="search"
          placeholder={S.search}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <span className="muted">{S.tagCount(file.tags.length)}</span>
        {Object.keys(edits).length > 0 && (
          <span className="changed-count">{S.changedCount(Object.keys(edits).length)}</span>
        )}
      </div>

      {file.error && (
        <div className="notice error">
          <strong>{S.readError}:</strong> {file.error}
        </div>
      )}
      {file.warnings.length > 0 && (
        <div className="notice warn">
          <strong>{S.warnings}:</strong> {file.warnings.join(" / ")}
        </div>
      )}
      {!file.error && writable && !fileWritable && <div className="notice info">{S.readOnlyFile}</div>}
      {Object.values(edits).some(isRemoveAll) && (
        <div className="notice warn remove-all">
          <span>{S.removeAllBanner}</span>
          <button className="small" onClick={() => onRevert(":all")}>
            {S.revert}
          </button>
        </div>
      )}

      <div className="tag-scroll">
        {groups.length === 0 && !file.error && <p className="muted pad">{S.noMatch}</p>}
        <table className="tag-table">
          <thead>
            <tr>
              <th className="col-tag">{S.colTag}</th>
              <th>{S.colValue}</th>
              <th className="col-actions" />
            </tr>
          </thead>
          {groups.map(([group, rows]) => {
            const isCollapsed = collapsed.has(group) && !query;
            return (
              <tbody key={group}>
                <tr className="group-row" onClick={() => toggle(group)}>
                  <td colSpan={3}>
                    <span className="chevron">{isCollapsed ? "▸" : "▾"}</span>
                    {group || "(なし)"} <span className="muted">({rows.length})</span>
                  </td>
                </tr>
                {!isCollapsed &&
                  rows.map((row, i) => {
                    const t = row.tag;
                    const key = tagKey(t);
                    const change = edits[key];
                    const editable = row.isNew || isTagEditable(file, t, writable);
                    const deleted = change && change.value === null;
                    const shown = change ? (change.value ?? "") : t.value;
                    const cls = deleted ? "deleted" : change ? "changed" : undefined;
                    return (
                      <tr key={`${key}-${i}`} className={cls}>
                        <td className="col-tag" title={row.isNew ? key : `${t.group}:${t.name}`}>
                          {row.isNew ? key : t.name}
                        </td>
                        {editing === key ? (
                          <td className="col-value editing">
                            <textarea
                              autoFocus
                              value={draft}
                              rows={Math.min(6, Math.max(1, draft.split("\n").length))}
                              onChange={(e) => setDraft(e.target.value)}
                              onBlur={() => commit(row)}
                              onKeyDown={(e) => {
                                if (e.key === "Enter" && !e.shiftKey) {
                                  e.preventDefault();
                                  commit(row);
                                } else if (e.key === "Escape") {
                                  e.preventDefault();
                                  e.stopPropagation();
                                  closing.current = true;
                                  setEditing(null);
                                }
                              }}
                            />
                            <span className="edit-hint">{S.editHint}</span>
                          </td>
                        ) : (
                          <td
                            className={`col-value${editable ? " editable" : ""}`}
                            title={change && !row.isNew ? `${S.colBefore}: ${t.value}` : undefined}
                            onDoubleClick={() => editable && !deleted && startEdit(row)}
                          >
                            <div className="value-clamp">
                              {deleted ? (
                                <span className="muted">{S.deleted} {t.value}</span>
                              ) : isBinary(t) && !row.isNew && !change ? (
                                <button
                                  className="binary-link"
                                  title={S.binaryOpen}
                                  onClick={() => onShowBinary(t)}
                                >
                                  {shown}
                                </button>
                              ) : (
                                shown
                              )}
                            </div>
                          </td>
                        )}
                        <td className="col-actions">
                          {isBinary(t) && !row.isNew && (
                            <button className="icon-btn" title={S.binaryOpen} aria-label={`${t.name}: ${S.binaryOpen}`} onClick={() => onShowBinary(t)}>
                              👁
                            </button>
                          )}
                          {editable && !deleted && editing !== key && (
                            <button className="icon-btn" title={S.edit} aria-label={`${t.name} を${S.edit}`} onClick={() => startEdit(row)}>
                              ✎
                            </button>
                          )}
                          {editable && !row.isNew && !deleted && (
                            <button
                              className="icon-btn"
                              title={S.deleteTag}
                              aria-label={`${t.name} を${S.deleteTag}`}
                              onClick={() => onChange({ group: t.group, name: t.name, value: null })}
                            >
                              🗑
                            </button>
                          )}
                          {change && (
                            <button className="icon-btn" title={S.revert} aria-label={`${t.name} を${S.revert}`} onClick={() => onRevert(key)}>
                              ↺
                            </button>
                          )}
                        </td>
                      </tr>
                    );
                  })}
              </tbody>
            );
          })}
        </table>
      </div>

      {fileWritable && <AddTagForm file={file} writable={writable} onAdd={onChange} />}
    </div>
  );
}

function AddTagForm({
  file,
  writable,
  onAdd,
}: {
  file: FileMetadata;
  writable: WritableInfo | null;
  onAdd: (c: TagChange) => void;
}) {
  const suggestions = isNativeFormat(file, writable) ? nativeTagSuggestions(file, writable) : COMMON_NEW_TAGS;
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    let parsed = parseTagInput(name);
    if (!parsed) return setError(S.addTagInvalid);
    if (!parsed.group && isNativeFormat(file, writable)) {
      // AVI / MKV / WebM はグループ省略時に、そのタグ名を書けるグループを補う
      const key = nativeTagSuggestions(file, writable).find((k) => k.split(":")[1].toLowerCase() === parsed!.name.toLowerCase());
      if (key) parsed = { group: key.split(":")[0], name: key.split(":")[1] };
    }
    if (!canWriteTag(file, writable, parsed.group, parsed.name)) return setError(S.addTagNotWritable(parsed.name));
    onAdd({ ...parsed, value });
    setName("");
    setValue("");
    setError(null);
  };

  return (
    <form className="add-tag" onSubmit={submit}>
      <input
        list="common-tags"
        placeholder={S.addTagName}
        aria-label={S.addTagName}
        value={name}
        onChange={(e) => setName(e.target.value)}
      />
      <datalist id="common-tags">
        {suggestions.map((t) => (
          <option key={t} value={t} />
        ))}
      </datalist>
      <input placeholder={S.addTagValue} aria-label={S.addTagValue} value={value} onChange={(e) => setValue(e.target.value)} />
      <button type="submit" disabled={!name.trim()}>
        {S.addTag}
      </button>
      {error && <span className="status-error">{error}</span>}
    </form>
  );
}
