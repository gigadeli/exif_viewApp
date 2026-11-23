import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getEmbeddedPreview } from "../api";
import { NATIVE_IMAGE_EXT, PLAYABLE_VIDEO_EXT, S } from "../strings";
import type { FileMetadata } from "../types";

type View =
  | { kind: "image"; src: string; embedded: boolean }
  | { kind: "video"; src: string }
  | { kind: "loading" }
  | { kind: "none"; message: string };

type Props = { file: FileMetadata };

export function Preview({ file }: Props) {
  const [view, setView] = useState<View>({ kind: "loading" });

  // WebView で表示できない形式は、ファイルに埋め込まれたプレビュー画像を探す
  const loadEmbedded = (path: string, cancelled: () => boolean) => {
    setView({ kind: "loading" });
    getEmbeddedPreview(path)
      .then((src) => {
        if (cancelled()) return;
        setView(src ? { kind: "image", src, embedded: true } : { kind: "none", message: S.previewNone });
      })
      .catch(() => !cancelled() && setView({ kind: "none", message: S.previewNone }));
  };

  useEffect(() => {
    let cancelled = false;
    const isCancelled = () => cancelled;
    // 保存後に同じパスを読み直したとき、キャッシュされた古い画像を出さないようにする
    const src = `${convertFileSrc(file.path)}?t=${file.loadedAt}`;
    if (NATIVE_IMAGE_EXT.test(file.fileName)) {
      setView({ kind: "image", src, embedded: false });
    } else if (PLAYABLE_VIDEO_EXT.test(file.fileName)) {
      setView({ kind: "video", src });
    } else {
      loadEmbedded(file.path, isCancelled);
    }
    return () => {
      cancelled = true;
    };
  }, [file.path, file.fileName, file.loadedAt]);

  return (
    <div className="preview">
      {view.kind === "loading" && <span className="muted">{S.previewLoading}</span>}
      {view.kind === "none" && <span className="muted">{view.message}</span>}
      {view.kind === "image" && (
        <>
          <img
            src={view.src}
            alt={file.fileName}
            onError={() => {
              // 拡張子は画像でも WebView が読めなかった場合 (壊れた JPEG など)
              if (!view.embedded) loadEmbedded(file.path, () => false);
              else setView({ kind: "none", message: S.previewNone });
            }}
          />
          {view.embedded && <span className="preview-note">{S.previewEmbedded}</span>}
        </>
      )}
      {view.kind === "video" && (
        <video
          src={view.src}
          controls
          preload="metadata"
          onError={() => setView({ kind: "none", message: S.previewVideoError })}
        />
      )}
    </div>
  );
}
