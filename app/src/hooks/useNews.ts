import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { getNewsList, getNewsById } from "../api/news";
import type { ApiCategory, ApiMarket, NewsListResponse } from "../api/types";

const PAGE_SIZE = 20;

export interface NewsFilters {
  market?: ApiMarket;
  category?: ApiCategory;
}

export function useNewsInfinite(filters: NewsFilters) {
  return useInfiniteQuery({
    queryKey: ["news", filters],
    queryFn: ({ pageParam }) =>
      getNewsList({
        market: filters.market,
        category: filters.category,
        page: pageParam as number,
        size: PAGE_SIZE,
      }),
    initialPageParam: 1,
    getNextPageParam: (last: NewsListResponse) => {
      const consumed = last.page * last.size;
      return consumed < last.total ? last.page + 1 : undefined;
    },
    staleTime: 60_000,
  });
}

export function useNewsDetail(id: string | undefined) {
  return useQuery({
    queryKey: ["news", id ?? ""],
    queryFn: () => getNewsById(id!),
    enabled: !!id,
    staleTime: 5 * 60_000,
  });
}
