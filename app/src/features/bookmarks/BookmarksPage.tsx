import { useNavigate, useParams } from "react-router-dom";
import { useBookmarks } from "../../hooks/useBookmarks";
import { useIsDesktop } from "../../lib/use-viewport";
import { apiToMarket, marketLabel } from "../../lib/market-enum";
import { ChainTimeline } from "./ChainTimeline";
import "./track.css";
import type { Bookmark } from "../../api/types";

function formatDate(iso: string): string {
  const d = new Date(iso.replace(" ", "T"));
  if (isNaN(d.getTime())) return iso;
  const m = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"][d.getMonth()];
  return `${d.getDate()} ${m}`;
}

function TrackCard({ b, selected, onClick }: { b: Bookmark; selected: boolean; onClick: () => void }) {
  const region = apiToMarket(b.market);
  return (
    <button
      type="button"
      className="track-card"
      aria-selected={selected}
      onClick={onClick}
    >
      <div className="track-card-meta">
        <span className="track-card-market">{marketLabel(region, "zh")} · {b.category}</span>
        <span className="track-card-evidence" data-empty={b.evidence_count === 0}>
          {b.evidence_count > 0 ? `${b.evidence_count} 证据` : "构建中"}
        </span>
        <span className="track-card-date">{formatDate(b.created_at)}</span>
      </div>
      <h3 className="track-card-title">{b.title}</h3>
      <p className="track-card-summary">{b.summary}</p>
      {b.keywords?.length > 0 && (
        <div className="track-card-keywords">
          {b.keywords.slice(0, 4).map((k) => (
            <span key={k} className="track-card-keyword">{k}</span>
          ))}
        </div>
      )}
    </button>
  );
}

function EmptyState() {
  return (
    <div className="track-empty">
      <div className="track-empty-glyph" aria-hidden="true">
        <svg viewBox="0 0 64 64" fill="none" stroke="currentColor" strokeWidth="1" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="32" cy="32" r="28"/>
          <circle cx="32" cy="32" r="18"/>
          <circle cx="32" cy="32" r="8"/>
          <circle cx="32" cy="32" r="2" fill="currentColor"/>
          <line x1="2" y1="32" x2="14" y2="32"/>
          <line x1="50" y1="32" x2="62" y2="32"/>
          <line x1="32" y1="2" x2="32" y2="14"/>
          <line x1="32" y1="50" x2="32" y2="62"/>
        </svg>
      </div>
      <p>
        还没有追踪事件<br/>
        在简报或新闻里点 <em style={{ color: "var(--accent)", fontStyle: "normal", fontFamily: "var(--f-mono)", fontSize: 12 }}>「追踪」</em> 按钮<br/>
        Agent 会自动溯源 5 级证据链
      </p>
    </div>
  );
}

export default function BookmarksPage() {
  const navigate = useNavigate();
  const { id } = useParams();
  const isDesktop = useIsDesktop();
  const { data: bookmarks, isPending, isError, error, refetch } = useBookmarks();

  // 移动端：有 :id 就只显示 chain；没 id 显示列表
  if (!isDesktop && id) {
    return (
      <section className="view" data-view="saved">
        <div className="track-view">
          <ChainTimeline bookmarkId={id} onAfterDelete={() => navigate("/track")} />
        </div>
      </section>
    );
  }

  // Desktop：永远显示双栏；当前选中由 :id 决定，没就默认第一个
  const selectedId =
    id ?? (isDesktop && bookmarks && bookmarks.length > 0 ? bookmarks[0].id : undefined);

  return (
    <section className="view" data-view="saved">
      <div className="track-view">

        <header className="track-head">
          <div className="track-title">
            <span className="track-title-en">Tracked</span>
            <span className="track-title-cn">追踪</span>
          </div>
          {bookmarks && bookmarks.length > 0 && (
            <span className="track-count">{bookmarks.length} 条 · {bookmarks.reduce((s, b) => s + (b.evidence_count || 0), 0)} 证据</span>
          )}
        </header>

        {isPending && (
          <div className="chain-loading">
            <span className="chain-loading-dot"></span>正在拉取追踪列表
          </div>
        )}

        {isError && (
          <div>
            <p style={{ color: "var(--accent)", fontFamily: "var(--f-mono)", fontSize: 11, letterSpacing: "0.14em" }}>
              {(error as Error)?.message || "无法加载列表"}
            </p>
            <button onClick={() => refetch()} className="chain-action" style={{ marginTop: 12 }}>重试</button>
          </div>
        )}

        {bookmarks && bookmarks.length === 0 && <EmptyState />}

        {bookmarks && bookmarks.length > 0 && (
          <div className="track-layout">
            <ul className="track-list">
              {bookmarks.map((b) => (
                <li key={b.id}>
                  <TrackCard
                    b={b}
                    selected={selectedId === b.id}
                    onClick={() => navigate(`/track/${b.id}`)}
                  />
                </li>
              ))}
            </ul>

            {isDesktop && selectedId && (
              <ChainTimeline
                key={selectedId}
                bookmarkId={selectedId}
                onAfterDelete={() => navigate("/track")}
              />
            )}
          </div>
        )}

      </div>
    </section>
  );
}
