// localStorage 封装，所有 key 走 anchormas:* 命名空间

const PREFIX = "anchormas:";

export function lsGet<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(PREFIX + key);
    if (raw === null) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

export function lsSet<T>(key: string, value: T): void {
  try {
    localStorage.setItem(PREFIX + key, JSON.stringify(value));
  } catch {
    /* quota */
  }
}

export function lsRemove(key: string): void {
  try {
    localStorage.removeItem(PREFIX + key);
  } catch { /* */ }
}

export const LS_KEYS = {
  theme: "theme",                 // 'light' | 'dark'
  briefLang: "brief-lang",        // 'zh' | 'en'
  pushTime: "push-time",          // 'HH:MM'
  shownMarkets: "shown-markets",  // MarketCode[]
  shownDims: "shown-dims",        // ApiCategory[]
  newsRegion: "news-region",      // MarketCode | 'all'
  briefRegion: "brief-region",    // MarketCode | 'all'
  tab: "tab",                     // string
  ttsVoice: "tts-voice",          // TtsVoice
} as const;
