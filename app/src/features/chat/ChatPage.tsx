// M4 — 对话页接通 API
// - GET/POST/DELETE sessions
// - GET/POST messages（12s 同步 LLM，显式 loading）
// - @news / @briefing 上下文（从 brief story / news 详情 "追问" 进来）
// - Mobile：左侧抽屉 session panel；Desktop：sidebar 已渲染 session list

import { useEffect, useRef, useState } from "react";
import { useLocation, useNavigate, useParams } from "react-router-dom";
import {
  useSessions,
  useMessages,
  useCreateSession,
  useStreamingSend,
  useDeleteSession,
} from "../../hooks/useChat";
import { useIsDesktop } from "../../lib/use-viewport";
import { SessionPanel } from "./SessionPanel";
import { Markdown } from "./markdown";
import "./chat-extras.css";
import type { ChatMessage, ChatSession, ChatContextType } from "../../api/types";

const SUGGEST_PROMPTS = [
  "今日哪个市场风险最高？",
  "对比中日韩本周变动",
  "越南那条对我品类影响多大？",
];

interface AskContext {
  contextType: ChatContextType;
  contextId: string;
  contextTitle: string;
}

function fmtTime(iso: string): string {
  const d = new Date(iso.replace(" ", "T"));
  if (isNaN(d.getTime())) return iso;
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

function MessageBlock({ m }: { m: ChatMessage }) {
  if (m.role === "user") {
    return (
      <div className="chat-block chat-block-user">
        <div className="chat-meta">
          <span className="chat-role">You</span>
          <time>{fmtTime(m.created_at)}</time>
        </div>
        <p className="chat-bubble">{m.content}</p>
      </div>
    );
  }
  return (
    <div className="chat-block chat-block-agent">
      <div className="chat-meta">
        <span className="chat-role">AnchorMAS</span>
        <time>{fmtTime(m.created_at)}</time>
      </div>
      <div className="chat-reply">
        <Markdown text={m.content} />
      </div>
    </div>
  );
}

function PendingBlock() {
  return (
    <div className="chat-block chat-block-agent">
      <div className="chat-meta">
        <span className="chat-role">AnchorMAS</span>
        <time>…</time>
      </div>
      <div className="chat-pending">
        <span className="chat-pending-radar" aria-hidden="true">
          <svg viewBox="0 0 22 22" fill="none" stroke="currentColor" strokeWidth="1">
            <circle cx="11" cy="11" r="9.5"/>
            <circle cx="11" cy="11" r="5.5"/>
            <circle cx="11" cy="11" r="1.5" fill="currentColor"/>
          </svg>
        </span>
        <span className="chat-pending-text">
          正在思考<span className="chat-pending-dots"></span>
        </span>
      </div>
    </div>
  );
}

export default function ChatPage() {
  const isDesktop = useIsDesktop();
  const navigate = useNavigate();
  const { sessionId: urlSid } = useParams();
  const location = useLocation();

  const { data: sessions = [] } = useSessions();
  const createSession = useCreateSession();
  const { send, streamingText, isStreaming, error: sendError } = useStreamingSend();
  const deleteSession = useDeleteSession();

  const currentSession: ChatSession | undefined =
    sessions.find((s) => s.id === urlSid) ?? (urlSid ? undefined : sessions[0]);
  const effectiveSid = currentSession?.id;
  const { data: messages = [] } = useMessages(effectiveSid);

  const [input, setInput] = useState("");
  const [panelOpen, setPanelOpen] = useState(false);
  const feedRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // 从 brief / news "追问" 进来，自动创会话
  useEffect(() => {
    const ask = (location.state as AskContext | null) ?? null;
    if (!ask) return;
    // 已经处理过的标记，避免 strict mode 双调用
    // 把 state 清掉防 reload 再触发
    navigate(location.pathname, { replace: true, state: null });
    createSession.mutate(
      {
        context_type: ask.contextType,
        context_id: ask.contextId,
        title: ask.contextTitle.slice(0, 28),
      },
      {
        onSuccess: (s) => {
          navigate(`/chat/${s.id}`, { replace: true });
        },
      },
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [location.state]);

  // URL 没 sid 但有 session → 同步到 url（避免刷新丢失）
  useEffect(() => {
    if (!urlSid && effectiveSid && !createSession.isPending) {
      navigate(`/chat/${effectiveSid}`, { replace: true });
    }
  }, [urlSid, effectiveSid, navigate, createSession.isPending]);

  // 滚到底（消息变化 + 流式 token 流入都触发）
  useEffect(() => {
    if (!feedRef.current) return;
    feedRef.current.scrollTop = feedRef.current.scrollHeight;
  }, [messages.length, isStreaming, streamingText]);

  // 输入框 autosize
  const onInputChange = (v: string) => {
    setInput(v);
    const el = textareaRef.current;
    if (el) {
      el.style.height = "auto";
      el.style.height = Math.min(el.scrollHeight, 120) + "px";
    }
  };

  const trySend = async (text?: string) => {
    const value = (text ?? input).trim();
    if (!value || isStreaming) return;
    let sid = effectiveSid;
    if (!sid) {
      const s = await createSession.mutateAsync({
        context_type: "free",
        title: value.slice(0, 28),
      });
      sid = s.id;
      navigate(`/chat/${sid}`, { replace: true });
    }
    setInput("");
    if (textareaRef.current) textareaRef.current.style.height = "auto";
    send(sid, value);
  };

  const onSelectSession = (id: string) => {
    navigate(`/chat/${id}`);
  };

  const onNewSession = async () => {
    const s = await createSession.mutateAsync({ context_type: "free" });
    navigate(`/chat/${s.id}`);
  };

  const onDeleteSession = (id: string) => {
    deleteSession.mutate(id, {
      onSuccess: () => {
        if (urlSid === id) navigate("/chat", { replace: true });
      },
    });
  };

  const isEmpty = (!effectiveSid || messages.length === 0) && !isStreaming;
  const ctxLabel = currentSession?.context_type && currentSession.context_type !== "free"
    ? (currentSession.context_type === "news" ? "新闻上下文 · News" : "简报上下文 · Briefing")
    : null;

  return (
    <section className="view" data-view="chat">
      <div className="chat-room">

        {/* mobile 顶 chrome；desktop 隐藏 */}
        <header className="chat-chrome">
          <button
            className="chat-icon-btn"
            type="button"
            aria-label="会话列表"
            onClick={() => setPanelOpen(true)}
          >
            <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
              <line x1="3" y1="5" x2="13" y2="5"/>
              <line x1="3" y1="8" x2="13" y2="8"/>
              <line x1="3" y1="11" x2="13" y2="11"/>
            </svg>
          </button>
          <span className="chat-title">{currentSession?.title ?? "对话"}</span>
          <span className="chat-chrome-spacer" aria-hidden="true"></span>
        </header>

        {/* 上下文卡（@news / @briefing） */}
        {ctxLabel && (
          <div className="chat-context">
            <span className="chat-context-label">{ctxLabel}</span>
            <p className="chat-context-headline">{currentSession?.title}</p>
          </div>
        )}

        {/* Feed */}
        {isEmpty && !createSession.isPending ? (
          <div className="chat-empty">
            <h2 className="chat-empty-title">
              <span className="chat-empty-en">How can I help today?</span>
              <span className="chat-empty-cn">今天想问什么？</span>
            </h2>
            <p className="chat-empty-sub">起手 · Try one of these</p>
            <div className="chat-prompts">
              {SUGGEST_PROMPTS.map((p) => (
                <button
                  key={p}
                  className="chat-prompt"
                  type="button"
                  onClick={() => trySend(p)}
                >{p}</button>
              ))}
            </div>
          </div>
        ) : (
          <div className="chat-feed" ref={feedRef}>
            {messages.map((m) => <MessageBlock key={m.id} m={m} />)}

            {/* 流式响应：第一帧前显示雷达占位；首 token 到达后切换为流式 Markdown */}
            {isStreaming && (
              streamingText ? (
                <div className="chat-block chat-block-agent" data-streaming="true">
                  <div className="chat-meta">
                    <span className="chat-role">AnchorMAS</span>
                    <time>streaming…</time>
                  </div>
                  <div className="chat-reply">
                    <Markdown text={streamingText} />
                    <span className="chat-cursor" aria-hidden="true"></span>
                  </div>
                </div>
              ) : <PendingBlock />
            )}

            {sendError && (
              <div className="chat-block chat-block-agent">
                <div className="chat-meta"><span className="chat-role">Error</span></div>
                <p className="chat-bubble" style={{ color: "var(--accent)" }}>{sendError.message}</p>
              </div>
            )}

            {createSession.isPending && !isStreaming && (
              <div className="chat-pending">
                <span className="chat-pending-radar" aria-hidden="true">
                  <svg viewBox="0 0 22 22" fill="none" stroke="currentColor" strokeWidth="1">
                    <circle cx="11" cy="11" r="9.5"/>
                    <circle cx="11" cy="11" r="5.5"/>
                    <circle cx="11" cy="11" r="1.5" fill="currentColor"/>
                  </svg>
                </span>
                <span className="chat-pending-text">正在创建会话<span className="chat-pending-dots"></span></span>
              </div>
            )}
          </div>
        )}

        {/* Input bar */}
        <footer className="chat-input-bar">
          <textarea
            ref={textareaRef}
            className="chat-input-field"
            rows={1}
            placeholder={isStreaming ? "AnchorMAS 正在思考…" : "向 AnchorMAS 提问…"}
            value={input}
            disabled={isStreaming}
            onChange={(e) => onInputChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                trySend();
              }
            }}
          />
          <button
            className="chat-send"
            type="button"
            aria-label="发送"
            onClick={() => trySend()}
            disabled={!input.trim() || isStreaming}
          >
            <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
              <path d="M8 13V3"/>
              <path d="M4 7l4-4 4 4"/>
            </svg>
          </button>
        </footer>
      </div>

      {/* 手机端抽屉 */}
      {!isDesktop && (
        <SessionPanel
          open={panelOpen}
          onClose={() => setPanelOpen(false)}
          sessions={sessions}
          currentId={effectiveSid}
          onSelect={onSelectSession}
          onNew={onNewSession}
          onDelete={onDeleteSession}
        />
      )}
    </section>
  );
}
