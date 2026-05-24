import { useEffect, useMemo } from "react";
import { NavLink, Outlet, useLocation } from "react-router-dom";
import "../styles/mobile.css";

// 路由 → prototype data-tab 值（沿用原型命名以让现有 CSS 生效）
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

export function MobileShell() {
  const { pathname } = useLocation();
  const dataTab = useMemo(() => routeTab(pathname), [pathname]);

  useEffect(() => {
    document.documentElement.setAttribute("data-shell", "mobile");
    document.body.setAttribute("data-shell", "mobile");
    return () => {
      document.documentElement.removeAttribute("data-shell");
      document.body.removeAttribute("data-shell");
    };
  }, []);

  return (
    <div className="mobile-app">
      <svg className="paper-grain" aria-hidden="true" xmlns="http://www.w3.org/2000/svg">
        <filter id="grain-m">
          <feTurbulence type="fractalNoise" baseFrequency="0.85" numOctaves={2} stitchTiles="stitch"/>
          <feColorMatrix values="0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 0.55 0"/>
        </filter>
        <rect width="100%" height="100%" filter="url(#grain-m)" opacity="0.06"/>
      </svg>

      <div className="app" data-tab={dataTab}>
        <main className="content">
          <Outlet />
        </main>

        <nav className="tabbar" aria-label="主导航">
          <span className="tabbar-bg" aria-hidden="true">
            <svg viewBox="0 0 800 100" preserveAspectRatio="none">
              <path d="M 0 100 L 0 22 L 320 22 C 348 22 358 2 400 2 C 442 2 452 22 480 22 L 800 22 L 800 100 Z"
                    strokeWidth={1} vectorEffect="non-scaling-stroke" />
            </svg>
          </span>

          <NavLink to="/news" className="tab-item" data-tab="market">
            <span className="tab-icon" aria-hidden="true">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="12" r="8.5"/><ellipse cx="12" cy="12" rx="3.6" ry="8.5"/><line x1="3.5" y1="12" x2="20.5" y2="12"/>
              </svg>
            </span>
            <span className="tab-label">新闻</span>
          </NavLink>

          <NavLink to="/chat" className="tab-item" data-tab="chat">
            <span className="tab-icon" aria-hidden="true">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
                <path d="M4 6.5A2.5 2.5 0 0 1 6.5 4h11A2.5 2.5 0 0 1 20 6.5v8A2.5 2.5 0 0 1 17.5 17H11l-4 3.5V17H6.5A2.5 2.5 0 0 1 4 14.5z"/>
                <circle cx="9" cy="10.5" r="0.9" fill="currentColor" stroke="none"/>
                <circle cx="12" cy="10.5" r="0.9" fill="currentColor" stroke="none"/>
                <circle cx="15" cy="10.5" r="0.9" fill="currentColor" stroke="none"/>
              </svg>
            </span>
            <span className="tab-label">对话</span>
          </NavLink>

          <NavLink to="/brief" className="tab-item tab-primary" data-tab="brief">
            <span className="tab-primary-ring" aria-hidden="true"></span>
            <span className="tab-icon" aria-hidden="true">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                <rect x="4" y="3" width="16" height="18" rx="1"/>
                <rect x="7" y="6.5" width="10" height="3" fill="currentColor" stroke="none" opacity="0.9"/>
                <line x1="7" y1="13" x2="17" y2="13"/>
                <line x1="7" y1="16.5" x2="13" y2="16.5"/>
              </svg>
            </span>
            <span className="tab-label">简报</span>
          </NavLink>

          <NavLink to="/track" className="tab-item" data-tab="saved">
            <span className="tab-icon" aria-hidden="true">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="12" r="7"/>
                <circle cx="12" cy="12" r="1.3" fill="currentColor" stroke="none"/>
                <line x1="12" y1="1" x2="12" y2="4"/><line x1="12" y1="20" x2="12" y2="23"/>
                <line x1="1" y1="12" x2="4" y2="12"/><line x1="20" y1="12" x2="23" y2="12"/>
              </svg>
            </span>
            <span className="tab-label">追踪</span>
          </NavLink>

          <NavLink to="/settings" className="tab-item tab-account" data-tab="settings">
            <span className="avatar avatar-tab" aria-hidden="true">
              <span className="avatar-initials">JY</span>
              <span className="avatar-dot"></span>
            </span>
            <span className="tab-label">设置</span>
          </NavLink>
        </nav>
      </div>
    </div>
  );
}
