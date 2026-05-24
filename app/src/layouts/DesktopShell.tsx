import { useEffect, useMemo } from "react";
import { NavLink, Outlet, useLocation, useNavigate, useParams } from "react-router-dom";
import "../styles/desktop.css";
import { RegionPicker } from "../features/brief/RegionPicker";
import { useBriefStore } from "../store/brief";
import { useNewsStore } from "../store/news";
import { useSessions, useCreateSession, useDeleteSession } from "../hooks/useChat";

const ROUTE_TO_TAB: Array<[RegExp, string]> = [
  [/^\/brief/,    "brief"],
  [/^\/chat/,     "chat"],
  [/^\/news/,     "market"],
  [/^\/track/,    "saved"],
  [/^\/settings/, "settings"],
];

function routeTab(pathname: string): string {
  for (const [re, tab] of ROUTE_TO_TAB) if (re.test(pathname)) return tab;
  return "brief";
}

interface NavBtn {
  to: string;
  tab: string;
  num: string;
  cn: string;
  en: string;
  icon: React.ReactNode;
}

const NAV: NavBtn[] = [
  {
    to: "/brief", tab: "brief", num: "01", cn: "简报", en: "Brief",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
        <rect x="4" y="3" width="16" height="18" rx="1"/>
        <rect x="7" y="6.5" width="10" height="3" fill="currentColor" stroke="none" opacity="0.85"/>
        <line x1="7" y1="13" x2="17" y2="13"/>
        <line x1="7" y1="16.5" x2="13" y2="16.5"/>
      </svg>
    ),
  },
  {
    to: "/chat", tab: "chat", num: "02", cn: "对话", en: "Dialogue",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
        <path d="M4 6.5A2.5 2.5 0 0 1 6.5 4h11A2.5 2.5 0 0 1 20 6.5v8A2.5 2.5 0 0 1 17.5 17H11l-4 3.5V17H6.5A2.5 2.5 0 0 1 4 14.5z"/>
        <circle cx="9" cy="10.5" r="0.9" fill="currentColor" stroke="none"/>
        <circle cx="12" cy="10.5" r="0.9" fill="currentColor" stroke="none"/>
        <circle cx="15" cy="10.5" r="0.9" fill="currentColor" stroke="none"/>
      </svg>
    ),
  },
  {
    to: "/news", tab: "market", num: "03", cn: "新闻", en: "News",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="12" cy="12" r="8.5"/>
        <ellipse cx="12" cy="12" rx="3.6" ry="8.5"/>
        <line x1="3.5" y1="12" x2="20.5" y2="12"/>
      </svg>
    ),
  },
  {
    to: "/track", tab: "saved", num: "04", cn: "追踪", en: "Track",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="12" cy="12" r="7"/>
        <circle cx="12" cy="12" r="1.3" fill="currentColor" stroke="none"/>
        <line x1="12" y1="1" x2="12" y2="4"/><line x1="12" y1="20" x2="12" y2="23"/>
        <line x1="1" y1="12" x2="4" y2="12"/><line x1="20" y1="12" x2="23" y2="12"/>
      </svg>
    ),
  },
  {
    to: "/settings", tab: "settings", num: "05", cn: "设置", en: "Settings",
    icon: (
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
        <circle cx="12" cy="12" r="3"/>
        <path d="M12 2v2.5M12 19.5V22M2 12h2.5M19.5 12H22M4.93 4.93l1.77 1.77M17.3 17.3l1.77 1.77M4.93 19.07l1.77-1.77M17.3 6.7l1.77-1.77"/>
      </svg>
    ),
  },
];

function relTime(iso: string): string {
  const d = new Date(iso.replace(" ", "T"));
  if (isNaN(d.getTime())) return iso;
  const diff = (Date.now() - d.getTime()) / 1000;
  if (diff < 60) return "now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}

export function DesktopShell() {
  const { pathname } = useLocation();
  const dataTab = useMemo(() => routeTab(pathname), [pathname]);
  const briefRegion = useBriefStore((s) => s.region);
  const setBriefRegion = useBriefStore((s) => s.setRegion);
  const newsRegion = useNewsStore((s) => s.region);
  const setNewsRegion = useNewsStore((s) => s.setRegion);
  const { data: sessions = [] } = useSessions();
  const createSession = useCreateSession();
  const deleteSession = useDeleteSession();
  const navigate = useNavigate();
  const { sessionId: urlSid } = useParams();

  useEffect(() => {
    document.documentElement.setAttribute("data-shell", "desktop");
    document.body.setAttribute("data-shell", "desktop");
    return () => {
      document.documentElement.removeAttribute("data-shell");
      document.body.removeAttribute("data-shell");
    };
  }, []);

  return (
    <div className="desktop-app">
      <svg className="paper-grain" aria-hidden="true" xmlns="http://www.w3.org/2000/svg">
        <filter id="grain-d">
          <feTurbulence type="fractalNoise" baseFrequency="0.85" numOctaves={2} stitchTiles="stitch"/>
          <feColorMatrix values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 0.55 0"/>
        </filter>
        <rect width="100%" height="100%" filter="url(#grain-d)" opacity="0.06"/>
      </svg>

      <div className="app" data-tab={dataTab}>
        <aside className="sidebar" aria-label="主导航">
          <div className="sidebar-brand">
            <span className="brand-mark" aria-hidden="true">
              <svg viewBox="0 0 40 40" fill="none" stroke="currentColor" strokeWidth="0.8" strokeLinecap="round">
                <circle cx="20" cy="20" r="17"/>
                <circle cx="20" cy="20" r="11"/>
                <circle cx="20" cy="20" r="5"/>
                <circle cx="20" cy="20" r="1.4" fill="currentColor" stroke="none"/>
                <line x1="20" y1="1" x2="20" y2="9"/>
                <line x1="20" y1="31" x2="20" y2="39"/>
                <line x1="1" y1="20" x2="9" y2="20"/>
                <line x1="31" y1="20" x2="39" y2="20"/>
              </svg>
            </span>
            <div className="brand-textblock">
              <span className="brand-name">AnchorMAS</span>
              <span className="brand-tag">Market Radar · 海外战情</span>
            </div>
          </div>

          <div className="nav-section-label">
            <span className="rule"></span><span className="rule-text">Sections</span><span className="rule"></span>
          </div>

          <nav className="sidebar-nav">
            {NAV.map((n) => (
              <div key={n.to}>
                <NavLink to={n.to} className="nav-item" data-tab={n.tab}>
                  <span className="nav-num">{n.num}</span>
                  <span className="nav-icon" aria-hidden="true">{n.icon}</span>
                  <span className="nav-text">
                    <span className="nav-label">{n.cn}</span>
                    <span className="nav-en">{n.en}</span>
                  </span>
                  <span className="nav-marker"></span>
                </NavLink>

                {n.tab === "brief" && dataTab === "brief" && (
                  <div className="nav-controls" data-controls="brief">
                    <button className="control-btn" type="button">
                      <span className="control-label">Date</span>
                      <span className="control-row">
                        <span className="control-value">今日</span>
                        <svg className="control-caret" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round"><path d="M2 4l3 3 3-3"/></svg>
                      </span>
                    </button>
                    <RegionPicker variant="control" value={briefRegion} onChange={setBriefRegion} />
                  </div>
                )}

                {n.tab === "market" && dataTab === "market" && (
                  <div className="nav-controls" data-controls="market">
                    <RegionPicker variant="control" value={newsRegion} onChange={setNewsRegion} />
                  </div>
                )}

                {n.tab === "chat" && dataTab === "chat" && (
                  <div className="nav-controls" data-controls="chat">
                    <button
                      className="nav-new-session"
                      type="button"
                      onClick={async () => {
                        const s = await createSession.mutateAsync({ context_type: "free" });
                        navigate(`/chat/${s.id}`);
                      }}
                      disabled={createSession.isPending}
                    >
                      <span className="nav-new-session-glyph">+</span>
                      <span>{createSession.isPending ? "Creating…" : "New session · 新对话"}</span>
                    </button>
                    <span className="nav-sessions-label">Recent</span>
                    <ul className="nav-sessions">
                      {sessions.length === 0 ? (
                        <li className="nav-sessions-empty">无</li>
                      ) : (
                        sessions.slice(0, 10).map((s) => (
                          <li
                            key={s.id}
                            className={`nav-session-item${s.id === urlSid ? " is-current" : ""}`}
                            onClick={() => navigate(`/chat/${s.id}`)}
                            style={{ display: "flex", alignItems: "center", gap: 8 }}
                          >
                            <span className="nav-session-item-title" style={{ flex: 1, minWidth: 0 }}>
                              {s.title}
                            </span>
                            <time className="nav-session-item-time">{relTime(s.updated_at)}</time>
                            <button
                              type="button"
                              className="nav-session-item-delete"
                              aria-label="删除"
                              onClick={(e) => {
                                e.stopPropagation();
                                deleteSession.mutate(s.id, {
                                  onSuccess: () => {
                                    if (s.id === urlSid) navigate("/chat", { replace: true });
                                  },
                                });
                              }}
                            >
                              <svg viewBox="0 0 12 12" width={11} height={11} fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round">
                                <path d="M3 3l6 6M9 3l-6 6"/>
                              </svg>
                            </button>
                          </li>
                        ))
                      )}
                    </ul>
                  </div>
                )}
              </div>
            ))}
          </nav>

          <div className="sidebar-foot">
            <button className="user-card" type="button" aria-label="账户">
              <span className="avatar" aria-hidden="true">
                <span className="avatar-initials">JY</span>
                <span className="avatar-dot"></span>
              </span>
              <span className="user-meta">
                <span className="user-name">Jie Ye</span>
                <span className="user-role">Cross-border Ops · 跨境运营</span>
              </span>
              <span className="user-chev" aria-hidden="true">
                <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M5 4l4 4-4 4"/>
                </svg>
              </span>
            </button>
          </div>
        </aside>

        <main className="content">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
