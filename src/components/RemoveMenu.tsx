import { useEffect, useRef, useState } from "react";
import { S } from "../strings";
import { ChevronDown } from "./Icons";

type Props = {
  disabled: boolean;
  hasGps: boolean;
  onRemoveGps: () => void;
  onRemoveAll: () => void;
};

/** 「メタデータ削除」メニュー。どちらも未保存の変更として追加するだけで、保存するまでファイルは変わらない */
export function RemoveMenu({ disabled, hasGps, onRemoveGps, onRemoveAll }: Props) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);

  const pick = (fn: () => void) => () => {
    setOpen(false);
    fn();
  };

  return (
    <div className="menu" ref={ref}>
      <button onClick={() => setOpen((o) => !o)} disabled={disabled}>
        {S.removeMenu} <ChevronDown />
      </button>
      {open && (
        <ul className="menu-list" role="menu">
          <li role="none">
            <button role="menuitem" disabled={!hasGps} onClick={pick(onRemoveGps)}>
              {S.removeGps}
            </button>
          </li>
          <li role="none">
            <button role="menuitem" onClick={pick(onRemoveAll)}>
              {S.removeAll}
            </button>
          </li>
        </ul>
      )}
    </div>
  );
}
