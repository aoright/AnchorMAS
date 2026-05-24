// === API_DOC.md 类型对齐 (Base: http://47.97.127.223:3200) ===

// ---------- 枚举 ----------
// 后端实际市场枚举范围比 doc 宽（已观察到 "India"），故放宽为开放枚举
export type ApiMarket =
  | "Global"
  | "China"
  | "Japan"
  | "Korea"
  | "SoutheastAsia"
  | "UnitedStates"
  | "India"
  | (string & {});

// 后端实际 category 范围超出 doc 5 个 enum（已观察到 "TechCraft"）
export type ApiCategory =
  | "Competition"
  | "Product"
  | "Social"
  | "Platform"
  | "Regulation"
  | "TechCraft"
  | (string & {});

export type ApiImpactType = "Opportunity" | "Risk" | "Attention";

export type Rating = 1 | 2 | 3 | 4 | 5;

// ---------- News / Event ----------
export interface NewsEvent {
  id: string;
  title: string;
  summary: string;
  market: ApiMarket;
  category: ApiCategory;
  impact_type: ApiImpactType;
  severity: Rating;
  urgency: Rating;
  confidence: Rating;
  source_urls: string[];
  analysis: string;
  created_at: string; // "YYYY-MM-DD HH:mm:ss"
}

export interface NewsRawSource {
  title: string;
  source_url: string;
  content: string;
}

export interface NewsDetail extends NewsEvent {
  raw_sources: NewsRawSource[];
}

export interface NewsListResponse {
  items: NewsEvent[];
  total: number;
  page: number;
  size: number;
}

export interface NewsListParams {
  market?: ApiMarket;
  category?: ApiCategory;
  page?: number;
  size?: number;
}

// ---------- Briefings ----------
// 后端实测形状（2026-05-23 验证）：
//   overview:  Record<ApiMarket, OverviewMarketBlock>
//   heatmap:   Record<Exclude<ApiMarket, "Global">, HeatmapMarketBlock>
//   recommendations: string[]
//   events:    NewsEvent[]

export interface OverviewKeyword {
  word: string;
  explanation: string;
  event_ids: string[];
}

export interface OverviewMarketBlock {
  keywords: OverviewKeyword[];
  summary: string;
}

export type BriefingOverview = Partial<Record<ApiMarket, OverviewMarketBlock>>;

export type HeatmapStatus = "稳定" | "关注" | "预警" | string;

export interface HeatmapMarketBlock {
  notes: string;
  status: HeatmapStatus;
}

export type BriefingHeatmap = Partial<Record<ApiMarket, HeatmapMarketBlock>>;

export interface BriefingListItem {
  id: string;
  date: string;
  overview: BriefingOverview;
  created_at: string;
}

export interface BriefingDetail {
  id: string;
  date: string;
  overview: BriefingOverview;
  heatmap: BriefingHeatmap;
  recommendations: string[];
  events: NewsEvent[];
  created_at: string;
}

// ---------- Chat ----------
export type ChatContextType = "free" | "news" | "briefing";
export type ChatRole = "user" | "assistant";

export interface ChatSession {
  id: string;
  title: string;
  context_type: ChatContextType;
  context_id: string | null;
  created_at: string;
  updated_at: string;
}

export interface ChatMessage {
  id: string;
  session_id: string;
  role: ChatRole;
  content: string;
  created_at: string;
}

export interface CreateSessionBody {
  title?: string;
  context_type?: ChatContextType;
  context_id?: string | null;
}

export interface SendMessageResponse {
  user_message: ChatMessage;
  ai_message: ChatMessage;
}

// ---------- Bookmarks ----------
export interface Bookmark {
  id: string;
  event_id: string;
  title: string;
  summary: string;
  market: ApiMarket;
  category: ApiCategory;
  keywords: string[];
  evidence_count: number;
  created_at: string;
}

export interface ChainNode {
  event_id: string;
  title: string;
  summary: string;
  market: ApiMarket;
  date: string;
  direction: "past" | "current" | "future";
  match_score: number;
  relation_description: string;
}

export interface BookmarkChainResponse {
  bookmark: Bookmark;
  chain: ChainNode[];
}

// ---------- Settings ----------
export interface ServerSettings {
  custom_keywords: string[];
  benchmark_companies: string[];
  updated_at: string;
}

// ---------- TTS ----------
export type TtsVoice = "longxiaochun_v3" | "longanyang" | "longxiaoxia_v3";

export interface TtsBody {
  text: string;
  voice?: TtsVoice;
}

// ---------- Error ----------
export interface ApiError {
  error: string;
}
