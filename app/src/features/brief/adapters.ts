// 把后端 BriefingDetail 转成原型 story-card 视图模型
// 不改字段语义、不造数据，只做枚举映射 + 排序 + 截断

import type {
  NewsEvent,
  ApiImpactType,
  HeatmapStatus,
  HeatmapMarketBlock,
  BriefingHeatmap,
  ApiMarket,
} from "../../api/types";
import {
  apiToMarket,
  marketLabel,
  type MarketCode,
} from "../../lib/market-enum";

export type Level = "high" | "watch" | "ok";

// impact_type → CSS data-level（控制 spine 颜色 + badge 色）
const IMPACT_TO_LEVEL: Record<ApiImpactType, Level> = {
  Risk: "high",
  Attention: "watch",
  Opportunity: "ok",
};

const STATUS_TO_LEVEL: Record<string, Level> = {
  预警: "high",
  关注: "watch",
  稳定: "ok",
};

export function impactToLevel(t: ApiImpactType): Level {
  return IMPACT_TO_LEVEL[t];
}

export function heatmapStatusToLevel(s: HeatmapStatus): Level {
  return STATUS_TO_LEVEL[s] ?? "ok";
}

// Story 视图模型——只保留 UI 渲染必要字段，原 event 通过 raw 传出去
export interface StoryVM {
  id: string;
  num: string;             // 01, 02, ...
  level: Level;
  region: MarketCode;
  marketCn: string;
  badge: ApiImpactType;
  sev: number;
  urg: number;
  conf: number;
  headline: string;
  outlook: string;         // = event.summary
  source_urls: string[];
  raw: NewsEvent;
}

// 排序：severity * urgency desc，并列再用 confidence desc，最后 created_at desc
function score(e: NewsEvent): number {
  return e.severity * e.urgency * 100 + e.confidence;
}

export function topEvents(events: NewsEvent[], n: number): NewsEvent[] {
  return [...events]
    .sort((a, b) => {
      const ds = score(b) - score(a);
      if (ds !== 0) return ds;
      return b.created_at.localeCompare(a.created_at);
    })
    .slice(0, n);
}

export function eventToStory(e: NewsEvent, index: number): StoryVM {
  const region = apiToMarket(e.market);
  return {
    id: e.id,
    num: String(index + 1).padStart(2, "0"),
    level: impactToLevel(e.impact_type),
    region,
    marketCn: marketLabel(region, "zh"),
    badge: e.impact_type,
    sev: e.severity,
    urg: e.urgency,
    conf: e.confidence,
    headline: e.title,
    outlook: e.summary,
    source_urls: e.source_urls,
    raw: e,
  };
}

// 截一句话作为 brief lead
export function firstSentence(text: string, maxChars = 160): string {
  if (!text) return "";
  const periods = ["。", "！", "？", "."];
  let cut = -1;
  for (const p of periods) {
    const idx = text.indexOf(p);
    if (idx !== -1 && (cut === -1 || idx < cut)) cut = idx;
  }
  if (cut === -1) {
    return text.length <= maxChars ? text : text.slice(0, maxChars) + "…";
  }
  const sentence = text.slice(0, cut + 1);
  return sentence.length <= maxChars ? sentence : sentence.slice(0, maxChars) + "…";
}

// Heatmap → 排序好的市场 chip 数据
export interface ClimateChipVM {
  region: MarketCode;
  marketCn: string;
  level: Level;
  status: HeatmapStatus;
  notes: string;
}

// 排序顺序（陶土序列）：cn / jp / kr / sea / us
const MARKET_ORDER: ApiMarket[] = [
  "China", "Japan", "Korea", "SoutheastAsia", "UnitedStates",
];

export function heatmapToChips(heatmap: BriefingHeatmap): ClimateChipVM[] {
  const out: ClimateChipVM[] = [];
  for (const m of MARKET_ORDER) {
    const block: HeatmapMarketBlock | undefined = heatmap?.[m];
    if (!block) continue;
    const region = apiToMarket(m);
    out.push({
      region,
      marketCn: marketLabel(region, "zh"),
      level: heatmapStatusToLevel(block.status),
      status: block.status,
      notes: block.notes,
    });
  }
  return out;
}
