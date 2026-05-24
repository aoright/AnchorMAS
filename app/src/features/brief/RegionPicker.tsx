// 复用的 region picker dropdown（受控）
// variant="pill"：mobile brief-meta 用（圆角胶囊）
// variant="control"：desktop sidebar nav-controls 用（堆栈式）

import { useEffect, useRef, useState } from "react";
import type { RegionFilter } from "../../store/brief";
import { marketLabel, type MarketCode } from "../../lib/market-enum";

interface Option {
  region: RegionFilter;
  cn: string;
  en: string;
}

const OPTIONS: Option[] = [
  { region: "all", cn: "全部市场", en: "All" },
  { region: "cn",  cn: marketLabel("cn"),  en: "CN" },
  { region: "jp",  cn: marketLabel("jp"),  en: "JP" },
  { region: "kr",  cn: marketLabel("kr"),  en: "KR" },
  { region: "sea", cn: marketLabel("sea"), en: "SEA" },
  { region: "us",  cn: marketLabel("us"),  en: "US" },
];

function regionLabelCn(r: RegionFilter): string {
  if (r === "all") return "全部市场";
  return marketLabel(r as MarketCode, "zh");
}

interface Props {
  value: RegionFilter;
  onChange: (r: RegionFilter) => void;
  variant?: "pill" | "control";
}

export function RegionPicker({ value, onChange, variant = "pill" }: Props) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onClick = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("click", onClick);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("click", onClick);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const labelCn = regionLabelCn(value);

  if (variant === "control") {
    return (
      <div className="region-picker" data-scope="brief" ref={rootRef}>
        <button
          className="control-btn"
          type="button"
          aria-haspopup="listbox"
          aria-expanded={open}
          onClick={(e) => { e.stopPropagation(); setOpen((v) => !v); }}
        >
          <span className="control-label">Region</span>
          <span className="control-row">
            <span className="control-value">{labelCn}</span>
            <svg className="control-caret" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round"><path d="M2 4l3 3 3-3"/></svg>
          </span>
        </button>
        {open && (
          <ul className="region-menu" role="listbox">
            {OPTIONS.map((o) => (
              <li
                key={o.region}
                role="option"
                aria-selected={o.region === value}
                onClick={() => { onChange(o.region); setOpen(false); }}
              >
                <span className="opt-cn">{o.cn}</span>
                <span className="opt-en">{o.en}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
    );
  }

  return (
    <div className="region-picker" ref={rootRef}>
      <button
        className="meta-pill"
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={(e) => { e.stopPropagation(); setOpen((v) => !v); }}
      >
        <svg className="meta-ico" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <circle cx="7" cy="7" r="5"/>
          <ellipse cx="7" cy="7" rx="2" ry="5"/>
          <line x1="2" y1="7" x2="12" y2="7"/>
        </svg>
        <span>{labelCn}</span>
        <svg className="control-caret" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M2 4l3 3 3-3"/></svg>
      </button>
      {open && (
        <ul className="region-menu" role="listbox">
          {OPTIONS.map((o) => (
            <li
              key={o.region}
              role="option"
              aria-selected={o.region === value}
              onClick={() => { onChange(o.region); setOpen(false); }}
            >
              <span className="opt-cn">{o.cn}</span>
              <span className="opt-en">{o.en}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
