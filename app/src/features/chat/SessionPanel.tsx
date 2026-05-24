// Mobile 会话列表抽屉（从左滑入）
// CSS 已经在 mobile.css 里：.session-sheet / .session-panel / animations

import { useEffect } from "react";
import type { ChatSession } from "../../api/types";

interface Props {
  open: boolean;
  onClose: () => void;
  sessions: ChatSession[];
  currentId?: string;
  onSelect: (id: string) => void;
  onNew: () => void;
  onDelete: (id: string) => void;
}

function relTime(iso: string): string {
  const d = new Date(iso.replace(" ", "T"));
  if (isNaN(d.getTime())) return iso;
  const diff = (Date.now() - d.getTime()) / 1000;
  if (diff < 60) return "just now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

export function SessionPanel({ open, onClose, sessions, currentId, onSelect, onNew, onDelete }: Props) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("keydown", onKey);
    document.body.style.overflow = "hidden";
    return () => {
      document.removeEventListener("keydown", onKey);
      document.body.style.overflow = "";
    };
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div className="session-sheet" data-role="session-sheet">
      <div className="source-backdrop" onClick={onClose} aria-hidden="true"></div>
      <aside className="session-panel" role="dialog" aria-label="会话列表">
        <header className="session-head">
          <span className="session-head-title">Sessions · 会话</span>
          <button className="source-close" type="button" onClick={onClose} aria-label="关闭">
            <svg viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round">
              <path d="M3 3l8 8M11 3l-8 8"/>
            </svg>
          </button>
        </header>
        <button className="session-new" type="button" onClick={() => { onNew(); onClose(); }}>
          <span className="session-new-glyph">+</span>
          <span>新对话 · New session</span>
        </button>
        <ul className="session-list">
          {sessions.length === 0 ? (
            <li className="session-list-empty">尚无会话</li>
          ) : (
            sessions.map((s) => (
              <li
                key={s.id}
                className={`session-list-item${s.id === currentId ? " is-current" : ""}`}
                onClick={() => { onSelect(s.id); onClose(); }}
                style={{ display: "flex", alignItems: "center", gap: 8 }}
              >
                <span className="session-list-item-title" style={{ flex: 1, minWidth: 0 }}>
                  {s.title}
                </span>
                <time className="session-list-item-time">{relTime(s.updated_at)}</time>
                <button
                  type="button"
                  className="session-list-item-delete"
                  aria-label="删除会话"
                  onClick={(e) => { e.stopPropagation(); onDelete(s.id); }}
                >
                  <svg viewBox="0 0 12 12" width={11} height={11} fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round">
                    <path d="M3 3l6 6M9 3l-6 6"/>
                  </svg>
                </button>
              </li>
            ))
          )}
        </ul>
      </aside>
    </div>
  );
}
