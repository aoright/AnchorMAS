import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// dev 用 proxy 绕过后端 CORS：浏览器请求 /app/* → vite 转发到 47.97.127.223:3200
// 生产 (Capacitor / 静态部署) 需要后端配 CORS，或者 webview 信任域名
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/app": {
        target: "http://47.97.127.223:3200",
        changeOrigin: true,
      },
    },
  },
});
