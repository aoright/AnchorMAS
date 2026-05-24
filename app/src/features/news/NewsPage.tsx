// M3 — 新闻页接通
// /app/news 分页 + region/category 筛选 + 无限滚动 + 点击进详情

import { useNavigate } from "react-router-dom";
import { useNewsStore, type NewsCategory } from "../../store/news";
import { useNewsInfinite } from "../../hooks/useNews";
import { useIsDesktop } from "../../lib/use-viewport";
import { apiToMarket, marketToApi, categoryLabel, ALL_CATEGORIES } from "../../lib/market-enum";
import { stripInlineIds } from "../brief/adapters";
import type { ApiCategory, NewsEvent } from "../../api/types";
import { publisherFromUrl } from "../../lib/host-publisher";
import "./news-extras.css";

const REGION_PILLS: { region: "all" | "cn" | "jp" | "kr" | "sea" | "us"; label: string }[] = [
  { region: "all", label: "All" },
  { region: "cn",  label: "中国" },
  { region: "jp",  label: "日本" },
  { region: "kr",  label: "韩国" },
  { region: "sea", label: "东南亚" },
  { region: "us",  label: "美国" },
];

function timeOnly(iso: string): string {
  // "2026-05-23 08:17:11" → "08:17"
  const m = /^\d{4}-\d{2}-\d{2}\s+(\d{2}):(\d{2})/.exec(iso);
  if (!m) return "";
  return `${m[1]}:${m[2]}`;
}

function NewsItem({ ev }: { ev: NewsEvent }) {
  const navigate = useNavigate();
  const region = apiToMarket(ev.market);
  const regionEn = (region as string).toUpperCase();
  const firstUrl = ev.source_urls?.[0];
  const sourceName = firstUrl ? publisherFromUrl(firstUrl) : "Source";
  return (
    <article
      className="news-item"
      data-region={region}
      data-impact={ev.impact_type}
      onClick={() => navigate(`/news/${ev.id}`)}
    >
      <div className="news-meta">
        <time className="news-time">{timeOnly(ev.created_at) || ev.created_at.slice(0, 10)}</time>
        <span className="news-source">{sourceName}</span>
        <span className="news-region">{regionEn}</span>
      </div>
      <h3 className="news-headline">{stripInlineIds(ev.title)}</h3>
      <p className="news-snippet">{stripInlineIds(ev.summary)}</p>
      <div className="news-impact-row">
        <span className="news-impact-badge">{ev.impact_type}</span>
        <span className="news-impact-metrics">
          <span className="news-impact-metric">Sev <b>{ev.severity}</b></span>
          <span className="news-impact-metric">Urg <b>{ev.urgency}</b></span>
          <span className="news-impact-metric">Conf <b>{ev.confidence}</b></span>
        </span>
        <span className="news-impact-metric" style={{ marginLeft: "auto" }}>
          {categoryLabel(ev.category)}
        </span>
      </div>
    </article>
  );
}

export default function NewsPage() {
  const isDesktop = useIsDesktop();
  const region = useNewsStore((s) => s.region);
  const setRegion = useNewsStore((s) => s.setRegion);
  const category = useNewsStore((s) => s.category);
  const setCategory = useNewsStore((s) => s.category && s.setCategory);

  const market = region !== "all" ? marketToApi(region as Exclude<typeof region, "all">) : undefined;
  const apiCategory = category !== "all" ? (category as ApiCategory) : undefined;

  const {
    data,
    isPending,
    isError,
    error,
    refetch,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
  } = useNewsInfinite({ market, category: apiCategory });

  const items = data?.pages.flatMap((p) => p.items) ?? [];
  const total = data?.pages[0]?.total ?? 0;

  return (
    <section className="view" data-view="market">
      <div className="news-view" data-active-region={region}>
        <header className="news-head">
          <div className="news-title-row">
            <h1 className="news-title">
              <span className="news-title-en">News Feed</span>
              <span className="news-title-cn">新闻流</span>
            </h1>
            {total > 0 && (
              <span style={{
                fontFamily: "var(--f-mono)",
                fontSize: 11,
                letterSpacing: "0.14em",
                color: "var(--ink-3)",
                marginLeft: 16,
              }}>
                {items.length} / {total}
              </span>
            )}
          </div>

          {/* mobile：region pills 在 head 内；desktop：region 在 sidebar nav-controls */}
          {!isDesktop && (
            <nav className="news-filters" aria-label="按地区筛选">
              {REGION_PILLS.map((p) => (
                <button
                  key={p.region}
                  className={`news-pill${region === p.region ? " is-active" : ""}`}
                  onClick={() => setRegion(p.region)}
                >
                  {p.label}
                </button>
              ))}
            </nav>
          )}

          {/* category pills 两端都显示 */}
          <div className="news-cat-filters" role="tablist" aria-label="按分类筛选">
            <button
              className={`news-cat-pill${category === "all" ? " is-active" : ""}`}
              onClick={() => setCategory && setCategory("all" as NewsCategory)}
            >All</button>
            {ALL_CATEGORIES.map((c) => (
              <button
                key={c}
                className={`news-cat-pill${category === c ? " is-active" : ""}`}
                onClick={() => setCategory && setCategory(c)}
              >
                {categoryLabel(c)}
              </button>
            ))}
          </div>
        </header>

        <div className="news-list">
          {isPending && (
            <div className="news-loadmore">正在拉取新闻…</div>
          )}

          {isError && (
            <div className="news-loadmore">
              <div style={{ color: "var(--accent)", marginBottom: 10 }}>
                {(error as Error)?.message || "无法加载新闻"}
              </div>
              <button onClick={() => refetch()}>重试</button>
            </div>
          )}

          {!isPending && !isError && items.length === 0 && (
            <div className="news-loadmore">
              当前筛选无结果
            </div>
          )}

          {items.map((ev) => <NewsItem key={ev.id} ev={ev} />)}

          {items.length > 0 && (
            <div className="news-loadmore">
              {hasNextPage ? (
                <button
                  onClick={() => fetchNextPage()}
                  disabled={isFetchingNextPage}
                >
                  {isFetchingNextPage ? "加载中…" : `加载更多 (${items.length} / ${total})`}
                </button>
              ) : (
                <span>— 全部 {total} 条已加载 —</span>
              )}
            </div>
          )}
        </div>
      </div>
    </section>
  );
}
