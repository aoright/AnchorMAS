import { api } from "./client";
import type { NewsDetail, NewsListParams, NewsListResponse } from "./types";

export function getNewsList(params: NewsListParams = {}) {
  return api<NewsListResponse>("/app/news", { query: { ...params } });
}

export function getNewsById(id: string) {
  return api<NewsDetail>(`/app/news/${id}`);
}
