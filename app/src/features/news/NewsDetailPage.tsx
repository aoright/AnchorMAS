import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { useNewsDetail } from "../../hooks/useNews";
import {
  useBookmarks,
  useCreateBookmark,
  useDeleteBookmark,
} from "../../hooks/useBookmarks";
import { apiToMarket, marketLabel, categoryLabel } from "../../lib/market-enum";
import { hostFromUrl, publisherFromUrl } from "../../lib/host-publisher";
import type { NewsRawSource } from "../../api/types";
import { stripInlineIds } from "../brief/adapters";
import "./news-extras.css";

// 把 analysis 里 [核查备注] / [核查警告] 这种内部 review 标注剥掉，
// 同时清掉 LLM 行内乱塞的 (ID: xxx) 引用
function cleanAnalysis(raw: string): string {
  const noReview = raw
    .split(/\n/)
    .filter((line) => !/^\[核查[备警]/.test(line.trim()))
    .join("\n")
    .trim();
  return stripInlineIds(noReview);
}

function RawSourceItem({ src, num }: { src: NewsRawSource; num: number }) {
  const [open, setOpen] = useState(num === 1);
  return (
    <div className="raw-source">
      <button
        type="button"
        className="raw-source-head"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <span className="raw-source-num">{String(num).padStart(2, "0")}</span>
        <span className="raw-source-title-line">
          <span className="raw-source-title">{src.title}</span>
          <span className="raw-source-host">{hostFromUrl(src.source_url)}</span>
        </span>
        <svg className="raw-source-caret" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
          <path d="M2 4l3 3 3-3"/>
        </svg>
      </button>
      {open && (
        <div className="raw-source-body">
          <p>{src.content}</p>
          <a
            className="raw-source-original"
            href={src.source_url}
            target="_blank"
            rel="noopener noreferrer"
          >
            View original
            <svg viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" width={11} height={11}>
              <path d="M5 3h6v6M11 3L5 9"/>
            </svg>
          </a>
        </div>
      )}
    </div>
  );
}

export default function NewsDetailPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const { data, isPending, isError, error, refetch } = useNewsDetail(id);
  const { data: bookmarks = [] } = useBookmarks();
  const createBookmark = useCreateBookmark();
  const deleteBookmark = useDeleteBookmark();

  if (isPending) {
    return (
      <section className="view" data-view="market">
        <div className="news-detail">
          <button className="news-detail-back" onClick={() => navigate(-1)}>← Back</button>
          <div style={{ color: "var(--ink-3)", fontFamily: "var(--f-mono)", fontSize: 11, letterSpacing: "0.14em" }}>
            加载中…
          </div>
        </div>
      </section>
    );
  }

  if (isError) {
    return (
      <section className="view" data-view="market">
        <div className="news-detail">
          <button className="news-detail-back" onClick={() => navigate(-1)}>← Back</button>
          <p style={{ color: "var(--accent)", marginBottom: 12 }}>
            {(error as Error)?.message || "无法加载新闻"}
          </p>
          <button onClick={() => refetch()} className="news-detail-action">重试</button>
        </div>
      </section>
    );
  }

  const ev = data!;
  const region = apiToMarket(ev.market);
  const cleanedAnalysis = cleanAnalysis(ev.analysis || "");
  const hasAnalysis = cleanedAnalysis && !/^Analysis unavailable/i.test(cleanedAnalysis);
  const existing = bookmarks.find((b) => b.event_id === ev.id);
  const isTracked = !!existing;
  const isTrackBusy = createBookmark.isPending || deleteBookmark.isPending;

  const toggleTrack = () => {
    if (isTrackBusy) return;
    if (existing) deleteBookmark.mutate(existing.id);
    else createBookmark.mutate(ev.id);
  };

  const ask = () => {
    navigate("/chat", {
      state: { contextType: "news", contextId: ev.id, contextTitle: ev.title },
    });
  };

  return (
    <section className="view" data-view="market">
      <div className="news-detail">
        <button className="news-detail-back" onClick={() => navigate(-1)}>
          <svg viewBox="0 0 10 10" width={10} height={10} fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
            <path d="M6 2L2 5l4 3"/>
          </svg>
          Back · 返回新闻流
        </button>

        <div className="news-detail-eyebrow">
          <span><b>{marketLabel(region, "zh")}</b></span>
          <span>{categoryLabel(ev.category)}</span>
          <span>{ev.impact_type}</span>
          <span>{ev.created_at.replace(" ", " · ")}</span>
        </div>

        <h1 className="news-detail-title">{stripInlineIds(ev.title)}</h1>

        <p className="news-detail-summary">{stripInlineIds(ev.summary)}</p>

        <div className="news-detail-metrics">
          <div className="news-detail-metric">
            <span className="news-detail-metric-label">Severity</span>
            <span className="news-detail-metric-chip" data-value={ev.severity}>{ev.severity}</span>
          </div>
          <div className="news-detail-metric">
            <span className="news-detail-metric-label">Urgency</span>
            <span className="news-detail-metric-chip" data-value={ev.urgency}>{ev.urgency}</span>
          </div>
          <div className="news-detail-metric">
            <span className="news-detail-metric-label">Confidence</span>
            <span className="news-detail-metric-chip" data-value={ev.confidence}>{ev.confidence}</span>
          </div>
          <div className="news-detail-metric" style={{ marginLeft: "auto" }}>
            <span className="news-detail-metric-label">Sources</span>
            <span className="news-detail-metric-value">{ev.source_urls?.length ?? 0}</span>
          </div>
        </div>

        {hasAnalysis && (
          <section className="news-detail-section">
            <header className="news-detail-section-head">
              <span className="news-detail-section-num">01</span>
              <span className="news-detail-section-title">Agent 分析</span>
              <span className="news-detail-section-en">Analysis</span>
            </header>
            <div className="news-detail-analysis">{cleanedAnalysis}</div>
          </section>
        )}

        <section className="news-detail-section">
          <header className="news-detail-section-head">
            <span className="news-detail-section-num">{hasAnalysis ? "02" : "01"}</span>
            <span className="news-detail-section-title">原文信源</span>
            <span className="news-detail-section-en">Sources</span>
          </header>

          {ev.raw_sources && ev.raw_sources.length > 0 ? (
            ev.raw_sources.map((src, i) => (
              <RawSourceItem key={src.source_url + i} src={src} num={i + 1} />
            ))
          ) : ev.source_urls && ev.source_urls.length > 0 ? (
            ev.source_urls.map((url, i) => (
              <a
                key={url + i}
                href={url}
                target="_blank"
                rel="noopener noreferrer"
                className="raw-source"
                style={{ display: "block", padding: "12px 14px", textDecoration: "none" }}
              >
                <div style={{ display: "flex", gap: 10 }}>
                  <span className="raw-source-num">{String(i + 1).padStart(2, "0")}</span>
                  <span style={{ flex: 1, minWidth: 0 }}>
                    <span className="raw-source-title">{publisherFromUrl(url)}</span>
                    <span className="raw-source-host" style={{ display: "block" }}>{hostFromUrl(url)}</span>
                  </span>
                </div>
              </a>
            ))
          ) : (
            <p style={{ color: "var(--ink-3)", padding: "12px 0" }}>无可用信源</p>
          )}
        </section>

        <div className="news-detail-actions">
          <button
            className="news-detail-action"
            data-tracked={isTracked || undefined}
            onClick={toggleTrack}
            disabled={isTrackBusy}
          >
            {isTrackBusy ? "…" : isTracked ? "已追踪" : "追踪"}
          </button>
          <button className="news-detail-action" onClick={ask}>
            追问 · Ask
          </button>
        </div>
      </div>
    </section>
  );
}
