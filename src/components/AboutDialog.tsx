import { useEffect, useMemo, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { S } from "../strings";

type Component = {
  source: "npm" | "cargo" | "bundled" | "data";
  name: string;
  version: string;
  license: string;
  url?: string;
  textIds: string[];
};
type Licenses = { components: Component[]; texts: Record<string, string> };

type Props = { exiftoolVersion: string | null; onClose: () => void };

const SOURCE_LABEL: Record<Component["source"], string> = {
  bundled: "同梱",
  data: "データ",
  npm: "画面",
  cargo: "本体",
};

/** アプリの情報と、同梱しているソフトウェアのライセンス */
export function AboutDialog({ exiftoolVersion, onClose }: Props) {
  const [appVersion, setAppVersion] = useState("");
  const [licenses, setLicenses] = useState<Licenses | null>(null);
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<string | null>(null);

  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => {});
    // ライセンス一覧は大きいので、開いたときに読み込む
    import("../generated/licenses.json").then((m) => setLicenses(m.default as Licenses));
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (licenses?.components ?? []).filter(
      (c) => !q || c.name.toLowerCase().includes(q) || c.license.toLowerCase().includes(q),
    );
  }, [licenses, query]);

  return (
    <div className="modal-backdrop">
      <div className="modal about" role="dialog" aria-modal="true" aria-labelledby="about-title">
        <h2 id="about-title">{S.appName}</h2>
        <dl className="info">
          <div>
            <dt>{S.aboutVersion}</dt>
            <dd>{appVersion}</dd>
          </div>
          <div>
            <dt>ExifTool</dt>
            <dd>{exiftoolVersion ?? "—"}（Phil Harvey, https://exiftool.org/）</dd>
          </div>
          <div>
            <dt>{S.aboutMap}</dt>
            <dd>© OpenStreetMap contributors（https://www.openstreetmap.org/copyright）</dd>
          </div>
        </dl>
        <p className="muted small-text">{S.aboutLicenseNote}</p>

        <input
          type="search"
          className="about-search"
          placeholder={S.aboutSearch}
          aria-label={S.aboutSearch}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <div className="license-list">
          {!licenses && <p className="muted pad">{S.loading}</p>}
          {list.map((c) => {
            const key = `${c.source}:${c.name}@${c.version}`;
            const isOpen = open === key;
            return (
              <div key={key} className="license-item">
                <button
                  className="license-head"
                  aria-expanded={isOpen}
                  onClick={() => setOpen(isOpen ? null : key)}
                >
                  <span className="chevron">{isOpen ? "▾" : "▸"}</span>
                  <span className="license-source">{SOURCE_LABEL[c.source]}</span>
                  <span className="license-name">
                    {c.name} <span className="muted">{c.version}</span>
                  </span>
                  <span className="muted">{c.license}</span>
                </button>
                {isOpen && (
                  <pre className="license-text">
                    {c.url ? `${c.url}\n\n` : ""}
                    {c.textIds.map((id) => licenses!.texts[id]).join("\n\n----------------------------------------\n\n")}
                  </pre>
                )}
              </div>
            );
          })}
        </div>
        <div className="modal-actions">
          <span className="muted small-text">{S.aboutCount(list.length)}</span>
          <span className="spacer" />
          <button className="primary" onClick={onClose} autoFocus>
            {S.close}
          </button>
        </div>
      </div>
    </div>
  );
}
