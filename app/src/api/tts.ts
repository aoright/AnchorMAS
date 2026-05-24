import { apiBlob } from "./client";
import type { TtsVoice } from "./types";

export function synthesizeSpeech(text: string, voice?: TtsVoice) {
  return apiBlob("/app/tts", {
    method: "POST",
    body: JSON.stringify({ text, ...(voice ? { voice } : {}) }),
  });
}
