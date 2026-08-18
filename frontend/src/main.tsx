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

// PWA：注册基础 Service Worker（离线缓存应用外壳，账本 API 仍走网络）。
if ("serviceWorker" in navigator) {
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

