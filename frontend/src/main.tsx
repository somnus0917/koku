import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
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
    navigator.serviceWorker.register("/sw.js").catch((error) => {
      console.error("Service worker registration failed", error);
    });
  });
}

