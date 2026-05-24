import { useCallback, useRef, useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  getSessions,
  createSession,
  getSessionMessages,
  sendMessageStream,
  deleteSession,
} from "../api/chat";
import type { ChatSession, ChatMessage, CreateSessionBody } from "../api/types";

const KEY_SESSIONS = ["chat", "sessions"] as const;
const KEY_MESSAGES = (sid: string) => ["chat", "sessions", sid, "messages"] as const;

export function useSessions() {
  return useQuery({
    queryKey: KEY_SESSIONS,
    queryFn: getSessions,
    staleTime: 30_000,
  });
}

export function useMessages(sessionId: string | undefined) {
  return useQuery({
    queryKey: KEY_MESSAGES(sessionId ?? ""),
    queryFn: () => getSessionMessages(sessionId!),
    enabled: !!sessionId,
    staleTime: Infinity, // 消息历史本地缓存，发新消息时手动 invalidate
  });
}

export function useCreateSession() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (body: CreateSessionBody) => createSession(body),
    onSuccess: (s) => {
      qc.invalidateQueries({ queryKey: KEY_SESSIONS });
      qc.setQueryData<ChatSession[]>(KEY_SESSIONS, (cur = []) => [s, ...cur.filter((x) => x.id !== s.id)]);
    },
  });
}

// 流式发送：直接面向组件订阅 streaming 文本
// 用法：const { send, streamingText, isStreaming, error } = useStreamingSend()
export function useStreamingSend() {
  const qc = useQueryClient();
  const [streamingText, setStreamingText] = useState("");
  const [isStreaming, setIsStreaming] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  const send = useCallback(async (sessionId: string, text: string) => {
    if (!sessionId || !text.trim()) return;
    if (abortRef.current) abortRef.current.abort();
    const ac = new AbortController();
    abortRef.current = ac;

    setIsStreaming(true);
    setStreamingText("");
    setError(null);

    const now = new Date().toISOString().replace("T", " ").slice(0, 19);

    // 乐观插入 user message (临时 id)
    const tempUserId = `temp-user-${Date.now()}`;
    qc.setQueryData<ChatMessage[]>(KEY_MESSAGES(sessionId), (cur = []) => [
      ...cur,
      { id: tempUserId, session_id: sessionId, role: "user", content: text, created_at: now },
    ]);

    let buffer = "";
    try {
      const { user_message_id, ai_message_id } = await sendMessageStream(
        sessionId,
        text,
        {
          onMetadata: (m) => {
            // 用真实 id 替换临时
            qc.setQueryData<ChatMessage[]>(KEY_MESSAGES(sessionId), (cur = []) =>
              cur.map((msg) => (msg.id === tempUserId ? { ...msg, id: m.user_message_id } : msg)),
            );
          },
          onChunk: (chunk) => {
            buffer += chunk;
            setStreamingText(buffer);
          },
          signal: ac.signal,
        },
      );

      // 流结束：把完整 ai message append
      qc.setQueryData<ChatMessage[]>(KEY_MESSAGES(sessionId), (cur = []) => [
        ...cur,
        {
          id: ai_message_id || `ai-${Date.now()}`,
          session_id: sessionId,
          role: "assistant",
          content: buffer,
          created_at: now,
        },
      ]);
      // 修正临时 user id (如果上面 metadata 没到，这里兜底)
      if (user_message_id) {
        qc.setQueryData<ChatMessage[]>(KEY_MESSAGES(sessionId), (cur = []) =>
          cur.map((msg) => (msg.id === tempUserId ? { ...msg, id: user_message_id } : msg)),
        );
      }

      qc.invalidateQueries({ queryKey: KEY_SESSIONS });
    } catch (e) {
      const err = e instanceof Error ? e : new Error(String(e));
      if (err.name !== "AbortError") {
        setError(err);
        // 回滚临时 user message（失败时不留半成品）
        qc.setQueryData<ChatMessage[]>(KEY_MESSAGES(sessionId), (cur = []) =>
          cur.filter((msg) => msg.id !== tempUserId),
        );
      }
    } finally {
      setStreamingText("");
      setIsStreaming(false);
      abortRef.current = null;
    }
  }, [qc]);

  const abort = useCallback(() => {
    abortRef.current?.abort();
  }, []);

  return { send, abort, streamingText, isStreaming, error };
}

export function useDeleteSession() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => deleteSession(id),
    onMutate: async (id) => {
      await qc.cancelQueries({ queryKey: KEY_SESSIONS });
      const prev = qc.getQueryData<ChatSession[]>(KEY_SESSIONS);
      qc.setQueryData<ChatSession[]>(KEY_SESSIONS, (cur = []) => cur.filter((s) => s.id !== id));
      return { prev };
    },
    onError: (_e, _id, ctx) => {
      if (ctx?.prev) qc.setQueryData(KEY_SESSIONS, ctx.prev);
    },
    onSettled: () => qc.invalidateQueries({ queryKey: KEY_SESSIONS }),
  });
}
