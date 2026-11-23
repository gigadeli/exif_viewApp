import { useEffect, useState } from "react";
import { fromInputValue, toInputValue, TZ_OPTIONS, type TzOffset } from "../dates";
import { isFileWritable } from "../editing";
import {
  dateFieldKeys,
  findByName,
  INFO_TAGS,
  mediaKind,
  readDate,
  readTextField,
  setDate,
  setTextField,
  textFieldKeys,
  type TextFieldId,
} from "../fields";
import { gpsKeys, isValidPoint, readGps, setGps, type GpsPoint } from "../gps";
import { S } from "../strings";
import type { FileEdits, FileMetadata, TagChange, WritableInfo } from "../types";

type Props = {
  file: FileMetadata;
  edits: FileEdits;
  writable: WritableInfo | null;
  onApply: (changes: TagChange[]) => void;
  onRevertKeys: (keys: string[]) => void;
  onShowMap: () => void;
  onHide: () => void;
};

/** 画像・動画で共通の「よく使う項目」フォーム。変更はタグ表と同じ「未保存の変更」に入る */
export function QuickEdit({ file, edits, writable, onApply, onRevertKeys, onShowMap, onHide }: Props) {
  const kind = mediaKind(file);
  const disabled = !isFileWritable(file, writable);
  const changed = (keys: string[]) => keys.some((k) => k in edits);

  const textRow = (id: TextFieldId, label: string, multiline = false) => {
    const keys = textFieldKeys(kind, id);
    return (
      <Field label={label} changed={changed(keys)} onRevert={() => onRevertKeys(keys)}>
        <TextInput
          label={label}
          value={readTextField(file, edits, kind, id)}
          multiline={multiline}
          disabled={disabled}
          onCommit={(v) => onApply(setTextField(file, edits, kind, id, v))}
        />
      </Field>
    );
  };

  const keywordKeys = textFieldKeys(kind, "keywords");
  const dateKeys = dateFieldKeys(file, kind);
  const gpsFieldKeys = gpsKeys(file, kind, edits);
  const gpsRemovals = setGps(file, edits, kind, null);
  const gps = readGps(file, edits, kind);

  return (
    <div className="quick">
      <div className="quick-head">
        <h2>{S.quickTitle}</h2>
        <button className="icon-btn visible" title={S.quickHide} aria-label={S.quickHide} onClick={onHide}>
          ×
        </button>
      </div>
      {disabled && writable && <p className="muted small-text">{S.fReadOnly}</p>}

      <Field label={S.fDate} changed={changed(dateKeys)} onRevert={() => onRevertKeys(dateKeys)}>
        <DateInput
          value={readDate(file, edits, kind)}
          allowNoTz={kind === "image"}
          disabled={disabled}
          onCommit={(dt, tz) => onApply(setDate(file, edits, kind, dt, tz))}
        />
        <small className="muted">{kind === "video" ? S.fDateVideoNote : S.fDateImageNote}</small>
      </Field>

      {textRow("title", S.fTitle)}
      {textRow("description", S.fDescription, true)}
      {textRow("artist", S.fArtist)}
      {textRow("copyright", S.fCopyright)}

      <Field label={S.fKeywords} changed={changed(keywordKeys)} onRevert={() => onRevertKeys(keywordKeys)}>
        <KeywordsInput
          value={readTextField(file, edits, kind, "keywords")}
          disabled={disabled}
          onCommit={(v) => onApply(setTextField(file, edits, kind, "keywords", v))}
        />
      </Field>

      <Field label={S.fGps} changed={changed(gpsFieldKeys)} onRevert={() => onRevertKeys(gpsFieldKeys)}>
        <GpsInput
          value={gps}
          disabled={disabled}
          onCommit={(p) => onApply(setGps(file, edits, kind, p))}
        />
        <div className="gps-actions">
          <button className="small" onClick={onShowMap}>
            {S.fGpsShowMap}
          </button>
          <button className="small" disabled={disabled || !gpsRemovals.length} onClick={() => onApply(gpsRemovals)}>
            {S.fGpsRemove}
          </button>
        </div>
      </Field>

      <h3>{S.fInfo}</h3>
      <dl className="info">
        {INFO_TAGS.map(({ label, names }) => {
          const values = names.map((n) => findByName(file, n)).filter(Boolean);
          if (values.length === 0) return null;
          return (
            <div key={label}>
              <dt>{label}</dt>
              <dd>{values.join(" / ")}</dd>
            </div>
          );
        })}
      </dl>
    </div>
  );
}

function Field({
  label,
  changed,
  onRevert,
  children,
}: {
  label: string;
  changed: boolean;
  onRevert: () => void;
  children: React.ReactNode;
}) {
  return (
    <div className={`field${changed ? " changed" : ""}`}>
      <div className="field-label">
        <span>{label}</span>
        {changed && (
          <>
            <span className="changed-count">{S.fChanged}</span>
            <button className="icon-btn visible" title={S.fRevert} aria-label={`${label}: ${S.fRevert}`} onClick={onRevert}>
              ↺
            </button>
          </>
        )}
      </div>
      {children}
    </div>
  );
}

/** 入力中は手元の下書きを持ち、フォーカスが外れたとき (1 行なら Enter でも) に確定する */
function TextInput({
  label,
  value,
  multiline,
  disabled,
  onCommit,
}: {
  label: string;
  value: string;
  multiline: boolean;
  disabled: boolean;
  onCommit: (v: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => draft !== value && onCommit(draft);

  const common = {
    "aria-label": label,
    value: draft,
    disabled,
    onBlur: commit,
  };
  return multiline ? (
    <textarea {...common} rows={3} onChange={(e) => setDraft(e.target.value)} />
  ) : (
    <input
      {...common}
      onChange={(e) => setDraft(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
        if (e.key === "Escape") setDraft(value);
      }}
    />
  );
}

function DateInput({
  value,
  allowNoTz,
  disabled,
  onCommit,
}: {
  value: ReturnType<typeof readDate>;
  allowNoTz: boolean;
  disabled: boolean;
  onCommit: (dt: ReturnType<typeof fromInputValue>, tz: TzOffset) => void;
}) {
  const current = value.dt ? toInputValue(value.dt) : "";
  const [draft, setDraft] = useState(current);
  useEffect(() => setDraft(current), [current]);

  const commitDate = () => {
    if (draft === current) return;
    const dt = fromInputValue(draft);
    if (dt || draft === "") onCommit(dt, value.tz);
  };

  const tzOptions = TZ_OPTIONS.includes(value.tz) || !value.tz ? TZ_OPTIONS : [value.tz, ...TZ_OPTIONS];
  return (
    <div className="date-row">
      <input
        type="datetime-local"
        step={1}
        aria-label={S.fDate}
        value={draft}
        disabled={disabled}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commitDate}
        onKeyDown={(e) => e.key === "Enter" && commitDate()}
      />
      <select
        aria-label={S.fTz}
        value={value.tz}
        disabled={disabled || !value.dt}
        onChange={(e) => value.dt && onCommit(value.dt, e.target.value)}
      >
        {allowNoTz && <option value="">{S.fTzNone}</option>}
        {tzOptions.map((tz) => (
          <option key={tz} value={tz}>
            UTC{tz}
          </option>
        ))}
      </select>
      <button
        className="icon-btn visible"
        title={S.fClearDate}
        aria-label={S.fClearDate}
        disabled={disabled || !value.dt}
        onClick={() => onCommit(null, value.tz)}
      >
        ×
      </button>
    </div>
  );
}

/** キーワードはチップで表示。値は ", " 区切りの 1 つの文字列 (書き込み時に -sep で分割) */
function KeywordsInput({
  value,
  disabled,
  onCommit,
}: {
  value: string;
  disabled: boolean;
  onCommit: (v: string) => void;
}) {
  const [input, setInput] = useState("");
  const words = value ? value.split(", ").filter(Boolean) : [];
  const commitWords = (list: string[]) => onCommit([...new Set(list)].join(", "));

  const add = () => {
    const added = input
      .split(/[,、]/)
      .map((w) => w.trim())
      .filter(Boolean);
    if (added.length) commitWords([...words, ...added]);
    setInput("");
  };

  return (
    <div className="keywords">
      {words.map((w) => (
        <span className="chip" key={w}>
          {w}
          {!disabled && (
            <button
              className="chip-x"
              aria-label={S.fRemoveKeyword(w)}
              onClick={() => commitWords(words.filter((x) => x !== w))}
            >
              ×
            </button>
          )}
        </span>
      ))}
      {!disabled && (
        <input
          aria-label={S.fKeywords}
          placeholder={S.fKeywordsPlaceholder}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === ",") {
              e.preventDefault();
              add();
            } else if (e.key === "Backspace" && !input && words.length) {
              commitWords(words.slice(0, -1));
            }
          }}
          onBlur={add}
        />
      )}
    </div>
  );
}

/** 緯度・経度・高度の数値入力。3 つの欄の外にフォーカスが移ったときに確定する */
function GpsInput({
  value,
  disabled,
  onCommit,
}: {
  value: GpsPoint | null;
  disabled: boolean;
  onCommit: (p: GpsPoint) => void;
}) {
  const fmt = (n: number | null | undefined, digits: number) =>
    n === null || n === undefined ? "" : String(Number(n.toFixed(digits)));
  const initial = { lat: fmt(value?.lat, 7), lon: fmt(value?.lon, 7), alt: fmt(value?.alt, 2) };
  const [draft, setDraft] = useState(initial);
  const [error, setError] = useState(false);
  const sig = `${initial.lat}|${initial.lon}|${initial.alt}`;
  useEffect(() => {
    setDraft(initial);
    setError(false);
  }, [sig]);

  const commit = (e: React.FocusEvent<HTMLDivElement>) => {
    if (e.currentTarget.contains(e.relatedTarget as Node)) return; // 欄の間の移動では確定しない
    if (`${draft.lat}|${draft.lon}|${draft.alt}` === sig) return;
    if (!draft.lat.trim() && !draft.lon.trim()) return;
    const p: GpsPoint = {
      lat: Number(draft.lat),
      lon: Number(draft.lon),
      alt: draft.alt.trim() === "" ? null : Number(draft.alt),
    };
    const ok = draft.lat.trim() !== "" && draft.lon.trim() !== "" && isValidPoint(p) && (p.alt === null || Number.isFinite(p.alt));
    setError(!ok);
    if (ok) onCommit(p);
  };

  const input = (k: "lat" | "lon" | "alt", label: string) => (
    <label className="gps-cell">
      <span>{label}</span>
      <input
        inputMode="decimal"
        aria-label={label}
        value={draft[k]}
        disabled={disabled}
        onChange={(e) => setDraft({ ...draft, [k]: e.target.value })}
        onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
      />
    </label>
  );

  return (
    <div className="gps-grid" onBlur={commit}>
      {input("lat", S.fLat)}
      {input("lon", S.fLon)}
      {input("alt", S.fAlt)}
      {error && <small className="status-error">{S.fGpsInvalid}</small>}
      {!value && !error && <small className="muted">{S.fGpsNone}</small>}
    </div>
  );
}
