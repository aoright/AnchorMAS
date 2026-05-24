import { api } from "./client";
import type { Bookmark, BookmarkChainResponse } from "./types";

export function getBookmarks() {
  return api<Bookmark[]>("/app/bookmarks");
}

export function createBookmark(eventId: string) {
  return api<Bookmark>("/app/bookmarks", {
    method: "POST",
    body: JSON.stringify({ event_id: eventId }),
  });
}

export function deleteBookmark(id: string) {
  return api<void>(`/app/bookmarks/${id}`, { method: "DELETE" });
}

export function getBookmarkChain(id: string) {
  return api<BookmarkChainResponse>(`/app/bookmarks/${id}/chain`);
}
