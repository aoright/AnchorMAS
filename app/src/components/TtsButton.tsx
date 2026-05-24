// 复用的 TTS 播放按钮
// variant="lead"：大号药丸（"听今日简报"），用在 brief 头部
// variant="ghost"：小图标，用在 story 卡 / news 详情等行内位置

import { useTtsStore } from "../store/tts";
import type { TtsVoice } from "../api/types";
import "./tts-button.css";

interface Props {
  ttsKey: string;
  text: string;
  variant?: "lead" | "ghost";
  labelIdle?: string;     // 仅 lead 用
  labelPlaying?: string;  // 仅 lead 用
  voice?: TtsVoice;       // 可选 override 全局选中的 voice (用于设置页 preview)
}

function SpeakerIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
      <path d="M2 6v4h2.5L8 13V3L4.5 6H2z" fill="currentColor" stroke="none"/>
      <path d="M11 5.5a3.2 3.2 0 0 1 0 5"/>
      <path d="M13 3.5a6 6 0 0 1 0 9"/>
    </svg>
  );
}

function StopIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="0">
      <rect x="4" y="4" width="8" height="8" rx="1.2" fill="currentColor"/>
    </svg>
  );
}

function LoadingRadar() {
  return (
    <span className="tts-loading" aria-hidden="true">
      <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1">
        <circle cx="8" cy="8" r="6"/>
        <circle cx="8" cy="8" r="3"/>
      </svg>
    </span>
  );
}

export function TtsButton({
  ttsKey,
  text,
  variant = "ghost",
  labelIdle = "听简报",
  labelPlaying = "停止",
  voice,
}: Props) {
  const activeKey = useTtsStore((s) => s.activeKey);
  const status = useTtsStore((s) => s.status);
  const play = useTtsStore((s) => s.play);
  const isMine = activeKey === ttsKey;
  const loading = isMine && status === "loading";
  const playing = isMine && status === "playing";

  const trimmed = text.trim();
  const disabled = !trimmed;

  const onClick = () => {
    if (disabled) return;
    void play(ttsKey, trimmed, voice);
  };

  if (variant === "lead") {
    return (
      <button
        type="button"
        className={`tts-lead${isMine ? " is-active" : ""}`}
        onClick={onClick}
        disabled={disabled}
        aria-label={playing ? labelPlaying : labelIdle}
      >
        <span className="tts-lead-icon">
          {loading ? <LoadingRadar /> : playing ? <StopIcon /> : <SpeakerIcon />}
        </span>
        <span className="tts-lead-label">
          {loading ? "合成中" : playing ? labelPlaying : labelIdle}
        </span>
      </button>
    );
  }

  return (
    <button
      type="button"
      className={`tts-ghost${isMine ? " is-active" : ""}`}
      onClick={onClick}
      disabled={disabled}
      aria-label={playing ? "停止朗读" : "朗读"}
      title={playing ? "停止朗读" : "朗读"}
    >
      {loading ? <LoadingRadar /> : playing ? <StopIcon /> : <SpeakerIcon />}
    </button>
  );
}
