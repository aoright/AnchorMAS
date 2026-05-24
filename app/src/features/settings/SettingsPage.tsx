// M5 视觉迁移 — DOM 1:1 抄 prototype；纯 UI 不接 API
// Theme 真正生效（影响 documentElement.dataset.theme）
// 其他控件本地状态 + localStorage 持久化，不实际改变行为

import { useEffect, useRef, useState } from "react";
import { lsGet, lsSet, LS_KEYS } from "../../lib/storage";
import { useTtsStore } from "../../store/tts";
import { TtsButton } from "../../components/TtsButton";
import type { TtsVoice } from "../../api/types";

type Theme = "light" | "dark";
type Lang = "zh" | "en";

const VOICE_OPTS: { value: TtsVoice; label: string; desc: string }[] = [
  { value: "longxiaochun_v3", label: "龙小淳", desc: "知识型女声" },
  { value: "longanyang",      label: "龙安洋", desc: "阳光大男孩" },
  { value: "longxiaoxia_v3",  label: "龙小夏", desc: "冷静权威女声" },
];

const VOICE_PREVIEW = "珠宝市场战略雷达今日检测到，周大福公布了二零二六年度全新战略蓝图。";

// 时区相对 CN（UTC+8）的小时偏移
const ZONES: Record<"cn" | "jp" | "kr" | "sea" | "us", number> = {
  cn: 0,
  jp: 1,
  kr: 1,
  sea: -1,
  us: -16,
};

function pad2(n: number) {
  return String(n).padStart(2, "0");
}

function computeClock(baseHHMM: string) {
  const [bh, bm] = baseHHMM.split(":").map(Number);
  if (Number.isNaN(bh) || Number.isNaN(bm)) return null;
  const baseMin = bh * 60 + bm;
  const out: Record<string, { h: number; m: number; day: number }> = {};
  for (const [zone, offHours] of Object.entries(ZONES)) {
    let t = baseMin + offHours * 60;
    let day = 0;
    while (t < 0) { t += 24 * 60; day -= 1; }
    while (t >= 24 * 60) { t -= 24 * 60; day += 1; }
    out[zone] = { h: Math.floor(t / 60), m: t % 60, day };
  }
  return out;
}

// =========================================================
// Time picker (custom 2-segment dropdown)
// =========================================================
function TimePicker({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [h, m] = value.split(":");
  const [openSeg, setOpenSeg] = useState<"hour" | "minute" | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!openSeg) return;
    const onClick = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpenSeg(null);
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") setOpenSeg(null); };
    document.addEventListener("click", onClick);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("click", onClick);
      document.removeEventListener("keydown", onKey);
    };
  }, [openSeg]);

  const HOURS = Array.from({ length: 24 }, (_, i) => pad2(i));
  const MIN_OPTIONS = ["00", "15", "30", "45"];

  return (
    <div className="time-picker" ref={rootRef} data-role="time-picker">
      <div className="time-seg-wrap" data-segment="hour">
        <button
          className="time-seg"
          type="button"
          aria-haspopup="listbox"
          aria-expanded={openSeg === "hour"}
          onClick={(e) => { e.stopPropagation(); setOpenSeg(openSeg === "hour" ? null : "hour"); }}
        >
          <span>{h}</span>
          <svg className="time-caret" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round"><path d="M2 4l3 3 3-3"/></svg>
        </button>
        {openSeg === "hour" && (
          <ul className="time-menu time-menu-hour" role="listbox">
            {HOURS.map((hh) => (
              <li
                key={hh}
                role="option"
                aria-selected={hh === h}
                data-value={hh}
                onClick={() => { onChange(`${hh}:${m}`); setOpenSeg(null); }}
              >{hh}</li>
            ))}
          </ul>
        )}
      </div>
      <span className="time-colon">:</span>
      <div className="time-seg-wrap" data-segment="minute">
        <button
          className="time-seg"
          type="button"
          aria-haspopup="listbox"
          aria-expanded={openSeg === "minute"}
          onClick={(e) => { e.stopPropagation(); setOpenSeg(openSeg === "minute" ? null : "minute"); }}
        >
          <span>{m}</span>
          <svg className="time-caret" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round"><path d="M2 4l3 3 3-3"/></svg>
        </button>
        {openSeg === "minute" && (
          <ul className="time-menu time-menu-minute" role="listbox">
            {MIN_OPTIONS.map((mm) => (
              <li
                key={mm}
                role="option"
                aria-selected={mm === m}
                data-value={mm}
                onClick={() => { onChange(`${h}:${mm}`); setOpenSeg(null); }}
              >{mm}</li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

// =========================================================
// Seg-control（亮/暗、中/英、5/10/15/20）
// =========================================================
interface SegOption<T extends string | number> { value: T; label: string }
function SegControl<T extends string | number>({
  options, value, onChange,
}: {
  options: SegOption<T>[];
  value: T;
  onChange: (v: T) => void;
}) {
  return (
    <div className="seg-control">
      {options.map((o) => (
        <button
          key={String(o.value)}
          className={`seg-btn${o.value === value ? " is-active" : ""}`}
          type="button"
          onClick={() => onChange(o.value)}
        >{o.label}</button>
      ))}
    </div>
  );
}

// =========================================================
// Multi-check pills
// =========================================================
function CheckPills<T extends string>({
  options, selected, onToggle,
}: {
  options: { value: T; label: string }[];
  selected: T[];
  onToggle: (v: T) => void;
}) {
  return (
    <div className="settings-checks">
      {options.map((o) => (
        <label key={o.value} className="check-pill">
          <input
            type="checkbox"
            value={o.value}
            checked={selected.includes(o.value)}
            onChange={() => onToggle(o.value)}
          />
          <span>{o.label}</span>
        </label>
      ))}
    </div>
  );
}

// =========================================================
// TTS Voice 选择
// =========================================================
function VoiceSection() {
  const voice = useTtsStore((s) => s.voice);
  const setVoice = useTtsStore((s) => s.setVoice);
  return (
    <section className="settings-section">
      <h2 className="settings-section-label">Voice · 语音播报</h2>
      <div className="settings-card">
        {VOICE_OPTS.map((v, i) => (
          <div className="settings-row" key={v.value} style={{ borderTop: i === 0 ? 0 : undefined }}>
            <label className="settings-row-main" style={{ display: "flex", alignItems: "center", gap: 12, cursor: "pointer" }}>
              <input
                type="radio"
                name="tts-voice"
                value={v.value}
                checked={voice === v.value}
                onChange={() => setVoice(v.value)}
                style={{
                  width: 16,
                  height: 16,
                  accentColor: "var(--accent)",
                  cursor: "pointer",
                }}
              />
              <span style={{ display: "flex", flexDirection: "column", gap: 2 }}>
                <span className="settings-row-label">{v.label}</span>
                <span className="settings-row-meta" style={{ fontSize: 11 }}>{v.desc}</span>
              </span>
            </label>
            <TtsButton ttsKey={`voice-preview-${v.value}`} text={VOICE_PREVIEW} voice={v.value} />
          </div>
        ))}
      </div>
    </section>
  );
}

// =========================================================
// SettingsPage
// =========================================================
const MARKET_OPTS = [
  { value: "cn",  label: "中国" },
  { value: "jp",  label: "日本" },
  { value: "kr",  label: "韩国" },
  { value: "sea", label: "东南亚" },
  { value: "us",  label: "美国" },
] as const;

const DIM_OPTS = [
  { value: "competition", label: "竞争" },
  { value: "product",     label: "产品" },
  { value: "platform",    label: "平台" },
  { value: "social",      label: "社媒" },
  { value: "regulation",  label: "法规" },
] as const;

export default function SettingsPage() {
  // 主题
  const [theme, setTheme] = useState<Theme>(() => lsGet<Theme>(LS_KEYS.theme, "light"));
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    lsSet(LS_KEYS.theme, theme);
  }, [theme]);

  // 简报语言（仅本地）
  const [lang, setLang] = useState<Lang>(() => lsGet<Lang>(LS_KEYS.briefLang, "zh"));
  useEffect(() => { lsSet(LS_KEYS.briefLang, lang); }, [lang]);

  // 最大新闻数（仅本地）
  const [maxStories, setMaxStories] = useState<number>(() => lsGet<number>("max-stories", 10));
  useEffect(() => { lsSet("max-stories", maxStories); }, [maxStories]);

  // 推送时间（仅本地）
  const [pushTime, setPushTime] = useState<string>(() => lsGet<string>(LS_KEYS.pushTime, "08:00"));
  useEffect(() => { lsSet(LS_KEYS.pushTime, pushTime); }, [pushTime]);
  const clock = computeClock(pushTime);

  // 关注市场（仅本地）
  type Market = (typeof MARKET_OPTS)[number]["value"];
  const [markets, setMarkets] = useState<Market[]>(() =>
    lsGet<Market[]>(LS_KEYS.shownMarkets, MARKET_OPTS.map((o) => o.value) as Market[]),
  );
  useEffect(() => { lsSet(LS_KEYS.shownMarkets, markets); }, [markets]);
  const toggleMarket = (v: Market) => {
    setMarkets((cur) => cur.includes(v) ? cur.filter((x) => x !== v) : [...cur, v]);
  };

  // 关注维度（仅本地）
  type Dim = (typeof DIM_OPTS)[number]["value"];
  const [dims, setDims] = useState<Dim[]>(() =>
    lsGet<Dim[]>(LS_KEYS.shownDims, DIM_OPTS.map((o) => o.value) as Dim[]),
  );
  useEffect(() => { lsSet(LS_KEYS.shownDims, dims); }, [dims]);
  const toggleDim = (v: Dim) => {
    setDims((cur) => cur.includes(v) ? cur.filter((x) => x !== v) : [...cur, v]);
  };

  return (
    <section className="view" data-view="settings">
      <article className="settings">

        <header className="settings-head">
          <h1 className="settings-title">
            <span className="settings-title-en">Settings</span>
            <span className="settings-title-cn">设置</span>
          </h1>
        </header>

        {/* Account */}
        <section className="settings-section">
          <h2 className="settings-section-label">Account · 账户</h2>
          <div className="settings-card">
            <div className="settings-account">
              <span className="avatar settings-avatar" aria-hidden="true">
                <span className="avatar-initials">JY</span>
                <span className="avatar-dot"></span>
              </span>
              <div className="settings-account-info">
                <span className="settings-account-name">Jie Ye</span>
                <span className="settings-account-role">Cross-border Ops · 跨境运营</span>
              </div>
            </div>
            <button className="settings-row settings-row-action" type="button">
              <span className="settings-row-label">退出登录 Sign out</span>
              <span className="settings-row-chev">›</span>
            </button>
          </div>
        </section>

        {/* Brief */}
        <section className="settings-section">
          <h2 className="settings-section-label">Brief · 简报</h2>
          <div className="settings-card">

            <div className="settings-row">
              <span className="settings-row-label">主题 Theme</span>
              <SegControl<Theme>
                value={theme}
                onChange={setTheme}
                options={[
                  { value: "light", label: "亮" },
                  { value: "dark",  label: "暗" },
                ]}
              />
            </div>

            <div className="settings-row">
              <span className="settings-row-label">语言 Language</span>
              <SegControl<Lang>
                value={lang}
                onChange={setLang}
                options={[
                  { value: "zh", label: "中文" },
                  { value: "en", label: "English" },
                ]}
              />
            </div>

            <div className="settings-row">
              <span className="settings-row-label">最大新闻数 Max stories</span>
              <SegControl<number>
                value={maxStories}
                onChange={setMaxStories}
                options={[
                  { value: 5,  label: "5" },
                  { value: 10, label: "10" },
                  { value: 15, label: "15" },
                  { value: 20, label: "20" },
                ]}
              />
            </div>

            <div className="settings-row settings-row-block">
              <div className="settings-row-main">
                <span className="settings-row-label">推送时间 Push time</span>
                <div className="settings-time-input">
                  <TimePicker value={pushTime} onChange={setPushTime} />
                  <span className="settings-row-meta">每日</span>
                </div>
              </div>
              {clock && (
                <div className="settings-clock" data-role="push-clock" aria-label="多地区对照">
                  <div className="clock-head">
                    <span>按 China time 计算</span>
                  </div>
                  {(["cn", "jp", "kr", "sea", "us"] as const).map((z) => {
                    const v = clock[z];
                    const day = v.day === -1 ? "前一日" : v.day === 1 ? "次日" : "";
                    const cn  = { cn: "中国", jp: "日本", kr: "韩国", sea: "东南亚", us: "美国" }[z];
                    const en  = { cn: "CN", jp: "JP", kr: "KR", sea: "ICT", us: "PST" }[z];
                    return (
                      <div className="clock-row" key={z}>
                        <span className="clock-region"><span className="clock-cn">{cn}</span> {en}</span>
                        <span className="clock-time">
                          {pad2(v.h)}:{pad2(v.m)}
                          {day && <> <span className="day-shift">{day}</span></>}
                        </span>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>

            <div className="settings-row settings-row-block">
              <span className="settings-row-label">关注市场 Markets</span>
              <CheckPills
                options={[...MARKET_OPTS]}
                selected={markets}
                onToggle={toggleMarket}
              />
            </div>

            <div className="settings-row settings-row-block">
              <span className="settings-row-label">关注维度 Dimensions</span>
              <CheckPills
                options={[...DIM_OPTS]}
                selected={dims}
                onToggle={toggleDim}
              />
            </div>

          </div>
        </section>

        {/* Voice */}
        <VoiceSection />

        {/* Sources */}
        <section className="settings-section">
          <h2 className="settings-section-label">Sources · 信源</h2>
          <div className="settings-card">
            <button className="settings-row settings-row-action" type="button">
              <div className="settings-row-main settings-row-stack">
                <span className="settings-row-label">激活信源 Active sources</span>
                <span className="settings-row-meta">11 unique · 5 regions · 4 languages</span>
              </div>
              <span className="settings-row-chev">›</span>
            </button>
          </div>
        </section>

        {/* About */}
        <section className="settings-section">
          <h2 className="settings-section-label">About · 关于</h2>
          <div className="settings-card">
            <div className="settings-row">
              <span className="settings-row-label">版本 Version</span>
              <span className="settings-row-meta">v 0.1.0 · build 0523</span>
            </div>
            <div className="settings-row">
              <span className="settings-row-label">Agent 模型 Model</span>
              <span className="settings-row-meta">Claude Opus 4.7</span>
            </div>
            <div className="settings-row">
              <span className="settings-row-label">上次生成 Last run</span>
              <span className="settings-row-meta">23 May · 08:30 +08:00</span>
            </div>
          </div>
        </section>

      </article>
    </section>
  );
}
