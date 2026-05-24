// M2 — 简报页接通后端 + 交互齐活
// 数据：/app/briefings/latest → 取 top 8 events 渲染 story 卡
// 交互：region picker 筛选 / story-more 展开 / 追踪持久化 / 追问跳 chat
// Mobile：meta pills + region picker 在 brief-head 内
// Desktop：date+region 控件在 sidebar nav-controls（由 DesktopShell 渲染）

import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useIsDesktop } from "../../lib/use-viewport";
import { useBriefingLatest } from "../../hooks/useBriefingLatest";
import { useBriefStore } from "../../store/brief";
import { useBookmarks, useCreateBookmark, useDeleteBookmark } from "../../hooks/useBookmarks";
import { RegionPicker } from "./RegionPicker";
import {
  topEvents,
  eventToStory,
  heatmapToChips,
  stripInlineIds,
  type StoryVM,
  type ClimateChipVM,
} from "./adapters";
import type { NewsEvent } from "../../api/types";
import { publisherFromUrl } from "../../lib/host-publisher";
import "./brief-extras.css";

const TOP_N = 8;

function formatBriefDate(iso: string): string {
  const d = new Date(iso + "T00:00:00");
  const wd = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][d.getDay()];
  const mo = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"][d.getMonth()];
  return `${wd} · ${d.getDate()} ${mo} ${d.getFullYear()}`;
}

function MoreIcon() {
  return (
    <>
      <span className="more-rule"></span>
      <span className="more-glyph">⋯</span>
      <span className="more-rule"></span>
    </>
  );
}

function TrackIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="8" cy="8" r="4.5"/>
      <circle cx="8" cy="8" r="1" fill="currentColor" stroke="none"/>
      <line x1="8" y1="1" x2="8" y2="2.6"/><line x1="8" y1="13.4" x2="8" y2="15"/>
      <line x1="1" y1="8" x2="2.6" y2="8"/><line x1="13.4" y1="8" x2="15" y2="8"/>
    </svg>
  );
}

function AskIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="8" cy="8" r="6.2"/>
      <path d="M6.3 6.3a1.7 1.7 0 0 1 3.4 0c0 1.2-1.7 1.7-1.7 1.7"/>
      <circle cx="8" cy="11.2" r="0.45" fill="currentColor"/>
    </svg>
  );
}

function MobileMetaPills({ dateText }: { dateText: string }) {
  const region = useBriefStore((s) => s.region);
  const setRegion = useBriefStore((s) => s.setRegion);
  return (
    <div className="brief-meta">
      <button className="meta-pill" type="button">
        <svg className="meta-ico" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <rect x="2" y="3" width="10" height="9" rx="1.2"/>
          <line x1="2" y1="6" x2="12" y2="6"/>
          <line x1="5" y1="2" x2="5" y2="4"/>
          <line x1="9" y1="2" x2="9" y2="4"/>
        </svg>
        <span>{dateText}</span>
        <svg className="control-caret" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M2 4l3 3 3-3"/></svg>
      </button>
      <RegionPicker variant="pill" value={region} onChange={setRegion} />
    </div>
  );
}

function MarketScan({ chips }: { chips: ClimateChipVM[] }) {
  if (!chips.length) return null;
  return (
    <div className="market-scan">
      {chips.map((c) => (
        <div className="scan-card" data-level={c.level} key={c.region}>
          <div className="scan-head">
            <span className="scan-market">{c.marketCn}</span>
            <span className="scan-status">{c.status}</span>
          </div>
          <p className="scan-notes">{c.notes || "今日无重大事件"}</p>
        </div>
      ))}
    </div>
  );
}

function InlineRecommendations({ items }: { items: string[] }) {
  const [open, setOpen] = useState(false);
  if (!items.length) return null;
  return (
    <div className="rec-wrap">
      <button
        type="button"
        className="rec-toggle"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <span className="rec-toggle-glyph">✦</span>
        <span className="rec-toggle-label">今日建议</span>
        <span className="rec-toggle-count">{items.length} 条</span>
        <svg className="rec-toggle-caret" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M2 4l3 3 3-3"/>
        </svg>
      </button>
      {open && (
        <div className="rec-panel">
          <ol className="rec-list">
            {items.map((text, i) => (
              <li className="rec-item" key={i}>
                <span className="rec-num">{String(i + 1).padStart(2, "0")}</span>
                <span className="rec-text">{text}</span>
              </li>
            ))}
          </ol>
        </div>
      )}
    </div>
  );
}

function Story({ s }: { s: StoryVM }) {
  const [open, setOpen] = useState(false);
  const navigate = useNavigate();
  const { data: bookmarks = [] } = useBookmarks();
  const createBookmark = useCreateBookmark();
  const deleteBookmark = useDeleteBookmark();
  const existing = bookmarks.find((b) => b.event_id === s.id);
  const isTracked = !!existing;
  const isWorking = createBookmark.isPending || deleteBookmark.isPending;

  const onTrack = () => {
    if (isWorking) return;
    if (existing) {
      deleteBookmark.mutate(existing.id);
    } else {
      createBookmark.mutate(s.id);
    }
  };

  const ask = () => {
    navigate("/chat", {
      state: { contextType: "news", contextId: s.id, contextTitle: s.headline },
    });
  };

  return (
    <article className="story" data-level={s.level} data-region={s.region} data-tracked={isTracked ? "true" : undefined}>
      <div className="story-num">{s.num}</div>
      <div className="story-main">
        <div className="story-top">
          <h2 className="story-market"><span className="market-cn">{s.marketCn}</span></h2>
          <div className="story-metrics">
            <span className="story-badge">{s.badge}</span>
            <span className="metric"><span className="metric-label">Sev</span><span className="metric-chip" data-value={s.sev}>{s.sev}</span></span>
            <span className="metric"><span className="metric-label">Urg</span><span className="metric-chip" data-value={s.urg}>{s.urg}</span></span>
            <span className="metric"><span className="metric-label">Conf</span><span className="metric-chip" data-value={s.conf}>{s.conf}</span></span>
          </div>
        </div>
        <h3 className="story-headline">{s.headline}</h3>
        <dl className="story-detail">
          <div className="detail-row"><dt>Outlook</dt><dd>{s.outlook}</dd></div>
        </dl>
        <button
          className="story-more"
          type="button"
          aria-expanded={open}
          aria-label={open ? "收起来源与操作" : "展开来源与操作"}
          onClick={() => setOpen((v) => !v)}
        >
          <MoreIcon />
        </button>
        <footer className="story-foot" hidden={!open}>
          <ol className="story-sources">
            {s.source_urls.map((url, i) => (
              <li key={url}>
                <a href={url} target="_blank" rel="noopener noreferrer">
                  <span className="src-num">{i + 1}</span>
                  <span className="src-name">{publisherFromUrl(url)}</span>
                </a>
              </li>
            ))}
          </ol>
          <div className="story-actions">
            <button
              className={`story-action${isTracked ? " is-tracked" : ""}`}
              type="button"
              onClick={onTrack}
              disabled={isWorking}
              aria-busy={isWorking}
            >
              <TrackIcon />
              <span>{isWorking ? "…" : isTracked ? "已追踪" : "追踪"}</span>
            </button>
            <button className="story-action" type="button" onClick={ask}>
              <AskIcon /><span>追问</span>
            </button>
          </div>
        </footer>
      </div>
    </article>
  );
}


function LoadingSkeleton() {
  return (
    <section className="view" data-view="brief">
      <article className="brief">
        <header className="brief-head">
          <h1 className="brief-title">
            <span className="brief-title-en">Daily Brief</span>
            <span className="brief-title-cn">今日简报</span>
          </h1>
          <p className="brief-lead" style={{ opacity: 0.5 }}>
            正在拉取今日简报…
          </p>
        </header>
        <div className="brief-orn" aria-hidden="true">
          <span className="rule"></span><span className="orn">◆</span><span className="rule"></span>
        </div>
      </article>
    </section>
  );
}

function ErrorView({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <section className="view" data-view="brief">
      <article className="brief">
        <header className="brief-head">
          <h1 className="brief-title">
            <span className="brief-title-en">Daily Brief</span>
            <span className="brief-title-cn">今日简报</span>
          </h1>
        </header>
        <div style={{
          margin: "40px 0",
          padding: "28px",
          border: "0.5px solid var(--hair-strong)",
          borderRadius: 12,
        }}>
          <div style={{
            fontFamily: "var(--f-mono)",
            fontSize: 10,
            letterSpacing: "0.18em",
            textTransform: "uppercase",
            color: "var(--accent)",
            marginBottom: 8,
          }}>Error</div>
          <p style={{ color: "var(--ink-2)", lineHeight: 1.6, marginBottom: 16 }}>{message}</p>
          <button
            onClick={onRetry}
            style={{
              padding: "6px 14px",
              border: "0.5px solid var(--hair-strong)",
              borderRadius: 999,
              fontFamily: "var(--f-mono)",
              fontSize: 10,
              letterSpacing: "0.12em",
              textTransform: "uppercase",
              color: "var(--ink-2)",
              background: "var(--paper)",
              cursor: "pointer",
            }}
          >Retry</button>
        </div>
      </article>
    </section>
  );
}

// 把 region filter 应用到 events
import type { MarketCode } from "../../lib/market-enum";
import { marketToApi } from "../../lib/market-enum";

function filterEventsByRegion(events: NewsEvent[] | null | undefined, region: MarketCode | "all"): NewsEvent[] {
  if (!events || !Array.isArray(events)) return [];
  if (region === "all") return events;
  const target = marketToApi(region);
  return events.filter((e) => e.market === target);
}

export default function BriefPage() {
  const isDesktop = useIsDesktop();
  const region = useBriefStore((s) => s.region);
  const { data, isPending, isError, error, refetch } = useBriefingLatest();

  if (isPending) return <LoadingSkeleton />;
  if (isError) {
    return <ErrorView message={(error as Error)?.message || "无法加载简报"} onRetry={() => refetch()} />;
  }

  const briefing = data!;
  const leadText = stripInlineIds(briefing.overview?.Global?.summary ?? "");
  const dateText = formatBriefDate(briefing.date);
  const chips = heatmapToChips(briefing.heatmap);

  const filtered = filterEventsByRegion(briefing.events, region);
  const stories = topEvents(filtered, TOP_N).map(eventToStory);

  return (
    <section className="view" data-view="brief">
      <article className="brief">

        <header className="brief-head">
          {!isDesktop && <MobileMetaPills dateText={dateText} />}

          <h1 className="brief-title">
            <span className="brief-title-en">Daily Brief</span>
            <span className="brief-title-cn">今日简报</span>
          </h1>

          {leadText && <p className="brief-lead">{leadText}</p>}

          <InlineRecommendations items={(briefing.recommendations ?? []).map(stripInlineIds)} />

          <MarketScan chips={chips} />
        </header>

        <div className="brief-orn" aria-hidden="true">
          <span className="rule"></span><span className="orn">◆</span><span className="rule"></span>
        </div>

        <div className="brief-stories">
          {stories.length === 0 ? (
            <p style={{ padding: "32px 0", textAlign: "center", color: "var(--ink-3)" }}>
              该市场今日无重大事件
            </p>
          ) : (
            stories.map((s) => <Story key={s.id} s={s} />)
          )}
        </div>

        <footer className="brief-foot">
          <span className="rule"></span>
          <span className="orn-text">
            End of Brief · 完
            {region === "all"
              ? ` · 取 ${stories.length}/${briefing.events?.length ?? 0} 条`
              : ` · ${stories.length}/${filtered.length} 条`}
          </span>
          <span className="rule"></span>
        </footer>

      </article>
    </section>
  );
}
