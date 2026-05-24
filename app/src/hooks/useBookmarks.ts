import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  getBookmarks,
  createBookmark,
  deleteBookmark,
  getBookmarkChain,
} from "../api/bookmarks";
import type { Bookmark } from "../api/types";

const KEY_LIST = ["bookmarks"] as const;
const KEY_CHAIN = (id: string) => ["bookmarks", id, "chain"] as const;

export function useBookmarks() {
  return useQuery({
    queryKey: KEY_LIST,
    queryFn: getBookmarks,
    staleTime: 30_000,
  });
}

export function useBookmarkChain(id: string | undefined) {
  return useQuery({
    queryKey: KEY_CHAIN(id ?? ""),
    queryFn: () => getBookmarkChain(id!),
    enabled: !!id,
    staleTime: 60_000,
    // 链路是异步构建的，未构完时后端可能返 chain 只含 current，需要重试拉新版本
    refetchInterval: (q) => {
      const data = q.state.data;
      if (!data) return false;
      // 如果 chain 只有 current，可能还没构完，60s 后再试
      const onlyCurrent = data.chain.length <= 1;
      return onlyCurrent ? 60_000 : false;
    },
  });
}

export function useCreateBookmark() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (eventId: string) => createBookmark(eventId),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: KEY_LIST });
    },
  });
}

export function useDeleteBookmark() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (bookmarkId: string) => deleteBookmark(bookmarkId),
    onMutate: async (id) => {
      await qc.cancelQueries({ queryKey: KEY_LIST });
      const prev = qc.getQueryData<Bookmark[]>(KEY_LIST);
      qc.setQueryData<Bookmark[]>(KEY_LIST, (cur = []) => cur.filter((b) => b.id !== id));
      return { prev };
    },
    onError: (_err, _id, ctx) => {
      if (ctx?.prev) qc.setQueryData(KEY_LIST, ctx.prev);
    },
    onSettled: () => {
      qc.invalidateQueries({ queryKey: KEY_LIST });
    },
  });
}
