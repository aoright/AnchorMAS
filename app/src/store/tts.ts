// 全局 TTS 控制：一次只播一段，新触发会停掉旧的
// blob 拉到后用 HTMLAudioElement 直接放，没 streaming playback 复杂度（后端是 chunked MP3 但浏览器照样吞）

import { create } from "zustand";
import { synthesizeSpeech } from "../api/tts";
import type { TtsVoice } from "../api/types";
import { lsGet, lsSet, LS_KEYS } from "../lib/storage";

type Status = "idle" | "loading" | "playing";

interface TtsState {
  // 当前选中的 voice (从 localStorage 恢复)
  voice: TtsVoice;
  setVoice: (v: TtsVoice) => void;

  // 当前播放的 key（用来让对应按钮亮起来 / 切换图标）
  activeKey: string | null;
  status: Status;
  error: string | null;

  // 操作
  play: (key: string, text: string, voiceOverride?: TtsVoice) => Promise<void>;
  stop: () => void;
}

// 全局唯一的 audio element（不放 React state，纯命令式）
let audioEl: HTMLAudioElement | null = null;
let blobUrl: string | null = null;

function tearDown() {
  if (audioEl) {
    audioEl.pause();
    audioEl.src = "";
    audioEl.onended = null;
    audioEl.onerror = null;
    audioEl = null;
  }
  if (blobUrl) {
    URL.revokeObjectURL(blobUrl);
    blobUrl = null;
  }
}

export const useTtsStore = create<TtsState>((set, get) => ({
  voice: lsGet<TtsVoice>(LS_KEYS.ttsVoice, "longxiaochun_v3"),
  setVoice: (v) => {
    lsSet(LS_KEYS.ttsVoice, v);
    set({ voice: v });
  },

  activeKey: null,
  status: "idle",
  error: null,

  play: async (key, text, voiceOverride) => {
    const { activeKey, status, voice: defaultVoice } = get();
    const voice = voiceOverride ?? defaultVoice;

    // 同 key 再次点击 → 切换暂停 / 停止
    if (activeKey === key && status !== "idle") {
      tearDown();
      set({ activeKey: null, status: "idle" });
      return;
    }

    // 不同 key 或之前是 idle → 切换
    tearDown();
    set({ activeKey: key, status: "loading", error: null });

    try {
      const blob = await synthesizeSpeech(text, voice);
      // 期间可能用户又点了别的（activeKey 已经变了），就别播这段
      if (get().activeKey !== key) return;

      blobUrl = URL.createObjectURL(blob);
      audioEl = new Audio(blobUrl);
      audioEl.onended = () => {
        tearDown();
        set({ activeKey: null, status: "idle" });
      };
      audioEl.onerror = () => {
        tearDown();
        set({ activeKey: null, status: "idle", error: "播放失败" });
      };
      await audioEl.play();
      // 仍可能在 await play() 期间被切走
      if (get().activeKey !== key) {
        tearDown();
        return;
      }
      set({ status: "playing" });
    } catch (e) {
      tearDown();
      set({
        activeKey: null,
        status: "idle",
        error: e instanceof Error ? e.message : String(e),
      });
    }
  },

  stop: () => {
    tearDown();
    set({ activeKey: null, status: "idle" });
  },
}));
