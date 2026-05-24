import { create } from "zustand";
import type { RegionFilter } from "./brief";
import type { ApiCategory } from "../api/types";
import { lsGet, lsSet, LS_KEYS } from "../lib/storage";

export type NewsCategory = ApiCategory | "all";

interface NewsState {
  region: RegionFilter;
  setRegion: (r: RegionFilter) => void;

  category: NewsCategory;
  setCategory: (c: NewsCategory) => void;
}

export const useNewsStore = create<NewsState>((set) => ({
  region: lsGet<RegionFilter>(LS_KEYS.newsRegion, "all"),
  setRegion: (r) => {
    lsSet(LS_KEYS.newsRegion, r);
    set({ region: r });
  },

  category: lsGet<NewsCategory>("news-category", "all"),
  setCategory: (c) => {
    lsSet("news-category", c);
    set({ category: c });
  },
}));
