import type { CapacitorConfig } from "@capacitor/cli";

const config: CapacitorConfig = {
  appId: "com.pandaax.anchormas",
  appName: "AnchorMAS",
  webDir: "dist",
  server: {
    androidScheme: "https",
  },
  android: {
    // 允许直连 IP 后端（HTTP 也走，不强制 https）
    allowMixedContent: true,
  },
  plugins: {
    // 接管 fetch/XHR → 走 native HTTP，绕开 WebView 的 CORS + cleartext 限制
    // ⚠️ 但 SSE 流式响应会变成"等完整 body 再一次性返回"——
    //    chat 在 mobile 上不会逐 token 流出，而是 12s 后一次性出现整段
    //    后端若加 Access-Control-Allow-Origin: * 则可以关闭 CapacitorHttp 恢复流式
    CapacitorHttp: {
      enabled: true,
    },
  },
};

export default config;
