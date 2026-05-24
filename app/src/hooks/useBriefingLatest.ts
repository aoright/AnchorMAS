import { useQuery } from "@tanstack/react-query";
import { getLatestBriefing } from "../api/briefings";
import type { ApiMarket } from "../api/types";

export function useBriefingLatest(market?: ApiMarket) {
  return useQuery({
    queryKey: ["briefing", "latest", market ?? "all"],
    queryFn: () => getLatestBriefing(market),
    staleTime: 5 * 60_000,
    retry: 1,
  });
}
