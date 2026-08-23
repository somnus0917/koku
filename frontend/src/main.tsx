import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./i18n";
import App from "./app/App";
import "./styles.css";

const root = document.getElementById("root");
if (!root) {
  throw new Error("Koku root element is missing");
}

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>
);

async function clearPreviewServiceWorker() {
  const registrations = await navigator.serviceWorker.getRegistrations();
  const cacheKeys = await caches.keys();
  const kokuCacheKeys = cacheKeys.filter((key) => key.startsWith("koku-"));

  await Promise.all([
    ...registrations.map((registration) => registration.unregister()),
    ...kokuCacheKeys.map((key) => caches.delete(key)),
  ]);

  // 首次执行时旧的 Service Worker 仍可能控制本页；清理后刷新一次才能重新请求
  // Vite 的 CSS 模块，避免把旧票根墙样式继续留在页面上。
  if (registrations.length > 0 || kokuCacheKeys.length > 0) {
    window.location.reload();
  }
}

if (import.meta.env.DEV && "serviceWorker" in navigator) {
  // 本地预览没有带内容哈希的资源 URL；若沿用生产缓存策略，旧 CSS 会被缓存优先
  // 命中，造成组件结构已更新、样式却停留在旧版本的错位显示。
  void clearPreviewServiceWorker();
} else if ("serviceWorker" in navigator) {
  // PWA：仅生产环境注册 Service Worker（离线缓存应用外壳，账本 API 仍走网络）。
  window.addEventListener("load", () => {
    navigator.serviceWorker
      .register("/sw.js")
      .then(() => {
        // 已有旧版 Service Worker 在控制页面时，等新版接管后自动刷新一次，
        // 避免停留在旧缓存的应用外壳上（旧 bundle 可能无法渲染新数据而白屏）。
        if (!navigator.serviceWorker.controller) return;
        navigator.serviceWorker.addEventListener("controllerchange", () => {
          window.location.reload();
        });
      })
      .catch((error) => {
        console.error("Service worker registration failed", error);
      });
  });
}
