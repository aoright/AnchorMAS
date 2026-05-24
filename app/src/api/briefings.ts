import { api } from "./client";
import type {
  BriefingDetail,
  BriefingListItem,
  ApiMarket,
} from "./types";

export interface BriefingListParams {
  page?: number;
  size?: number;
}

export interface BriefingListResponse {
  items: BriefingListItem[];
  total: number;
  page: number;
  size: number;
}

export function getBriefingList(params: BriefingListParams = {}) {
  return api<BriefingListResponse>("/app/briefings", { query: { ...params } });
}

export function getLatestBriefing(market?: ApiMarket) {
  return api<BriefingDetail>("/app/briefings/latest", {
    query: { market },
  });
}

export function getBriefingById(id: string, market?: ApiMarket) {
  return api<BriefingDetail>(`/app/briefings/${id}`, {
    query: { market },
  });
}
