import type { ApiMarket, ApiCategory, ApiImpactType } from "../api/types";

// 前端用的 short code（保留原型小写习惯）
// 后端实测会返超出 doc 6 个 enum 的市场（如 India），所以 MarketCode 也放宽
export type MarketCode = "cn" | "jp" | "kr" | "sea" | "us" | "global" | "in" | (string & {});
export type ImpactCode = "risk" | "opportunity" | "attention";

const MARKET_TO_API: Record<string, ApiMarket> = {
  cn: "China",
  jp: "Japan",
  kr: "Korea",
  sea: "SoutheastAsia",
  us: "UnitedStates",
  global: "Global",
  in: "India" as ApiMarket,
};

const API_TO_MARKET: Record<string, MarketCode> = {
  China: "cn",
  Japan: "jp",
  Korea: "kr",
  SoutheastAsia: "sea",
  UnitedStates: "us",
  Global: "global",
  India: "in",
};

const MARKET_LABEL_ZH: Record<string, string> = {
  cn: "中国",
  jp: "日本",
  kr: "韩国",
  sea: "东南亚",
  us: "美国",
  global: "全球",
  in: "印度",
};

const MARKET_LABEL_EN: Record<string, string> = {
  cn: "China",
  jp: "Japan",
  kr: "Korea",
  sea: "Southeast Asia",
  us: "United States",
  global: "Global",
  in: "India",
};

export function marketToApi(code: MarketCode): ApiMarket {
  return (MARKET_TO_API[code as string] ?? code) as ApiMarket;
}

export function apiToMarket(api: ApiMarket): MarketCode {
  return (API_TO_MARKET[api as string] ?? api) as MarketCode;
}

export function marketLabel(code: MarketCode, lang: "zh" | "en" = "zh"): string {
  const map = lang === "zh" ? MARKET_LABEL_ZH : MARKET_LABEL_EN;
  const key = code as string;
  return map[key] ?? key;
}

// 简报筛选用 — 仅 doc 标准 5 市场（mobile pill / desktop sidebar 都用这套）
export const ALL_MARKETS: MarketCode[] = ["cn", "jp", "kr", "sea", "us"];
export const ALL_MARKETS_WITH_GLOBAL: MarketCode[] = ["cn", "jp", "kr", "sea", "us", "global"];

// ---------- category ----------
// 后端 enum 范围会超 doc，未知值兜底显示原 key
const CATEGORY_LABEL_ZH: Record<string, string> = {
  Competition: "竞争",
  Product: "产品",
  Social: "社会",
  Platform: "平台",
  Regulation: "法规",
  TechCraft: "技术",
};

export function categoryLabel(c: ApiCategory): string {
  return CATEGORY_LABEL_ZH[c as string] ?? (c as string);
}

export const ALL_CATEGORIES: ApiCategory[] = [
  "Competition",
  "Product",
  "Social",
  "Platform",
  "Regulation",
  "TechCraft",
];

// ---------- impact ----------
const IMPACT_TO_CODE: Record<ApiImpactType, ImpactCode> = {
  Risk: "risk",
  Opportunity: "opportunity",
  Attention: "attention",
};

const IMPACT_LABEL: Record<ImpactCode, string> = {
  risk: "风险",
  opportunity: "机会",
  attention: "关注",
};

export function impactToCode(api: ApiImpactType): ImpactCode {
  return IMPACT_TO_CODE[api];
}

export function impactLabel(code: ImpactCode): string {
  return IMPACT_LABEL[code];
}
