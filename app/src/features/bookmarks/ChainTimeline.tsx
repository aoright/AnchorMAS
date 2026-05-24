import { useBookmarkChain, useDeleteBookmark } from "../../hooks/useBookmarks";
import { apiToMarket, marketLabel } from "../../lib/market-enum";
import { useNavigate } from "react-router-dom";
import type { ChainNode } from "../../api/types";

interface Props {
  bookmarkId: string;
  onAfterDelete?: () => void;
}

function formatDate(iso: string): string {
  const d = new Date(iso.replace(" ", "T"));
  if (isNaN(d.getTime())) return iso;
  const m = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"][d.getMonth()];
  return `${d.getDate()} ${m} ${d.getFullYear()}`;
}

const DIRECTION_LABEL: Record<ChainNode["direction"], string> = {
  past: "前因 · Past",
  current: "当前 · Current",
  future: "后续 · Future",
};

function Node({ n }: { n: ChainNode }) {
  const region = apiToMarket(n.market);
  const isEdgeNode = n.direction !== "current";
  return (
    <li className="chain-node" data-direction={n.direction}>
      <span className="chain-node-tag">{DIRECTION_LABEL[n.direction]}</span>
      <span className="chain-node-date">{formatDate(n.date)}</span>
      <span className="chain-node-market">{marketLabel(region, "zh")}</span>
      <h3 className="chain-node-title">{n.title}</h3>
      <p className="chain-node-summary">{n.summary}</p>
      {n.relation_description && (
        <div className="chain-node-relation">
          <span className="chain-node-relation-glyph">¶</span>
          <span className="chain-node-relation-text">{n.relation_description}</span>
        </div>
      )}
      {isEdgeNode && (
        <div className="chain-node-score">
          Match score: <b>{(n.match_score * 100).toFixed(0)}%</b>
        </div>
      )}
    </li>
  );
}

export function ChainTimeline({ bookmarkId, onAfterDelete }: Props) {
  const { data, isPending, isError, error, refetch } = useBookmarkChain(bookmarkId);
  const deleteBookmark = useDeleteBookmark();
  const navigate = useNavigate();

  if (isPending) {
    return (
      <div className="chain-panel">
        <div className="chain-loading">
          <span className="chain-loading-dot"></span>
          正在拉取证据链
        </div>
      </div>
    );
  }
  if (isError) {
    return (
      <div className="chain-panel">
        <p style={{ color: "var(--accent)", fontFamily: "var(--f-mono)", fontSize: 11, letterSpacing: "0.14em" }}>
          {(error as Error)?.message || "无法加载证据链"}
        </p>
        <button onClick={() => refetch()} className="chain-action" style={{ marginTop: 12 }}>重试</button>
      </div>
    );
  }

  const { bookmark, chain } = data!;
  // 按日期排序 past → current → future
  const sortedChain = [...chain].sort((a, b) => a.date.localeCompare(b.date));
  const hasMore = chain.length > 1;
  const region = apiToMarket(bookmark.market);

  const ask = () => {
    navigate("/chat", {
      state: { contextType: "news", contextId: bookmark.event_id, contextTitle: bookmark.title },
    });
  };

  const remove = () => {
    deleteBookmark.mutate(bookmark.id, {
      onSuccess: () => {
        onAfterDelete?.();
      },
    });
  };

  return (
    <div className="chain-panel">
      <button className="chain-back" type="button" onClick={() => navigate("/track")}>
        <svg viewBox="0 0 10 10" width={10} height={10} fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
          <path d="M6 2L2 5l4 3"/>
        </svg>
        Back · 返回列表
      </button>

      <header className="chain-head">
        <div className="chain-head-eyebrow">Evidence Chain · 证据链</div>
        <h2 className="chain-head-title">{bookmark.title}</h2>
        <div className="chain-head-meta">
          <span><b>{marketLabel(region, "zh")}</b> · {bookmark.category}</span>
          <span>{chain.length} 个节点</span>
          <span>收藏于 {formatDate(bookmark.created_at)}</span>
        </div>
      </header>

      <ul className="chain-timeline">
        {sortedChain.map((n) => <Node key={n.event_id + n.direction} n={n} />)}
      </ul>

      {!hasMore && (
        <p className="chain-empty-hint">
          证据链仍在构建中 — 后台正在做 5 级递归溯源，刷新或稍候即可看到关联的前因 / 后续事件。
        </p>
      )}

      <div className="chain-actions">
        <button className="chain-action" type="button" onClick={ask}>追问 · Ask</button>
        <button className="chain-action" type="button" data-variant="danger" onClick={remove} disabled={deleteBookmark.isPending}>
          {deleteBookmark.isPending ? "删除中…" : "取消追踪"}
        </button>
      </div>
    </div>
  );
}
