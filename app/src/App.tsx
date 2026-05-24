import { useEffect } from "react";
import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import { MobileShell } from "./layouts/MobileShell";
import { DesktopShell } from "./layouts/DesktopShell";
import BriefPage from "./features/brief/BriefPage";
import NewsPage from "./features/news/NewsPage";
import NewsDetailPage from "./features/news/NewsDetailPage";
import ChatPage from "./features/chat/ChatPage";
import BookmarksPage from "./features/bookmarks/BookmarksPage";
import SettingsPage from "./features/settings/SettingsPage";
import { lsGet, LS_KEYS } from "./lib/storage";
import { useIsDesktop } from "./lib/use-viewport";

function useThemeRestore() {
  useEffect(() => {
    const theme = lsGet<"light" | "dark">(LS_KEYS.theme, "light");
    document.documentElement.dataset.theme = theme;
  }, []);
}

function Routed() {
  const isDesktop = useIsDesktop();
  const Shell = isDesktop ? DesktopShell : MobileShell;
  return (
    <Routes>
      <Route element={<Shell />}>
        <Route index element={<Navigate to="/brief" replace />} />
        <Route path="/brief" element={<BriefPage />} />
        <Route path="/news" element={<NewsPage />} />
        <Route path="/news/:id" element={<NewsDetailPage />} />
        <Route path="/chat" element={<ChatPage />} />
        <Route path="/chat/:sessionId" element={<ChatPage />} />
        <Route path="/track" element={<BookmarksPage />} />
        <Route path="/track/:id" element={<BookmarksPage />} />
        <Route path="/settings" element={<SettingsPage />} />
        <Route path="*" element={<Navigate to="/brief" replace />} />
      </Route>
    </Routes>
  );
}

export default function App() {
  useThemeRestore();
  return (
    <BrowserRouter>
      <Routed />
    </BrowserRouter>
  );
}
