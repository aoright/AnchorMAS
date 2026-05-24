import { api, API_BASE, ApiHttpError } from "./client";
import type {
  ChatMessage,
  ChatSession,
  CreateSessionBody,
  SendMessageResponse,
} from "./types";

export function getSessions() {
  return api<ChatSession[]>("/app/chat/sessions");
}

export function createSession(body: CreateSessionBody = {}) {
  return api<ChatSession>("/app/chat/sessions", {
    method: "POST",
    body: JSON.stringify(body),
  });
}

export function getSessionMessages(sessionId: string) {
  return api<ChatMessage[]>(`/app/chat/sessions/${sessionId}/messages`);
}

// Legacy 非流式（保留作 fallback）
export function sendMessage(sessionId: string, message: string) {
  return api<SendMessageResponse>(
    `/app/chat/sessions/${sessionId}/messages`,
    {
      method: "POST",
      body: JSON.stringify({ message }),
    },
  );
}

export function deleteSession(sessionId: string) {
  return api<void>(`/app/chat/sessions/${sessionId}`, { method: "DELETE" });
}

// ============ SSE 流式 send ============
// 后端格式（已验证）：
//   Content-Type: text/event-stream
//   event 1: data: {"type":"metadata","ai_message_id":"...","user_message_id":"...","session_id":"..."}
//   event N: data: {"type":"content","content":"片段"}
//   连接关闭 = 流结束（无显式 done 事件）

export interface StreamMetadata {
  user_message_id: string;
  ai_message_id: string;
  session_id: string;
}

export interface StreamCallbacks {
  onMetadata?: (m: StreamMetadata) => void;
  onChunk?: (text: string) => void;
  signal?: AbortSignal;
}

function buildStreamUrl(sessionId: string): string {
  const path = `/app/chat/sessions/${sessionId}/messages`;
  if (!API_BASE) return path;
  return API_BASE.replace(/\/+$/, "") + path;
}

export async function sendMessageStream(
  sessionId: string,
  message: string,
  cb: StreamCallbacks = {},
): Promise<{ user_message_id: string; ai_message_id: string; full_content: string }> {
  const res = await fetch(buildStreamUrl(sessionId), {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "text/event-stream" },
    body: JSON.stringify({ message, stream: true }),
    signal: cb.signal,
  });
  if (!res.ok || !res.body) {
    const text = await res.text().catch(() => "");
    throw new ApiHttpError(res.status, text, `HTTP ${res.status}`);
  }

  let userId = "";
  let aiId = "";
  let buffer = "";
  let fullContent = "";

  const reader = res.body.getReader();
  const decoder = new TextDecoder();

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });

      // SSE 事件以 \n\n 分隔
      let idx: number;
      while ((idx = buffer.indexOf("\n\n")) >= 0) {
        const eventBlock = buffer.slice(0, idx);
        buffer = buffer.slice(idx + 2);
        for (const line of eventBlock.split("\n")) {
          if (!line.startsWith("data:")) continue;
          const raw = line.slice(5).trimStart();
          if (!raw) continue;
          let payload: { type?: string; content?: string; user_message_id?: string; ai_message_id?: string; session_id?: string };
          try { payload = JSON.parse(raw); } catch { continue; }
          if (payload.type === "metadata") {
            userId = payload.user_message_id ?? "";
            aiId = payload.ai_message_id ?? "";
            cb.onMetadata?.({
              user_message_id: userId,
              ai_message_id: aiId,
              session_id: payload.session_id ?? sessionId,
            });
          } else if (payload.type === "content") {
            const chunk = payload.content ?? "";
            fullContent += chunk;
            cb.onChunk?.(chunk);
          }
        }
      }
    }
  } finally {
    try { reader.releaseLock(); } catch { /* */ }
  }

  return { user_message_id: userId, ai_message_id: aiId, full_content: fullContent };
}
