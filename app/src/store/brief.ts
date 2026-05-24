// Brief 页相关的跨组件状态（mobile pill + desktop sidebar 共享）
// 追踪状态走 API（useBookmarks），不在这里保存
import { create } from "zustand";
import type { MarketCode } from "../lib/market-enum";
import { lsGet, lsSet, LS_KEYS } from "../lib/storage";

export type RegionFilter = MarketCode | "all";

interface BriefState {
  region: RegionFilter;
  setRegion: (r: RegionFilter) => void;
}

export const useBriefStore = create<BriefState>((set) => ({
  region: lsGet<RegionFilter>(LS_KEYS.briefRegion, "all"),
  setRegion: (r) => {
    lsSet(LS_KEYS.briefRegion, r);
    set({ region: r });
  },
}));
