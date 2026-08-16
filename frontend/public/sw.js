// Koku 基础 Service Worker：缓存应用外壳与静态资源，离线时回退到缓存。
// API 请求始终走网络（不缓存账本数据），避免离线时展示过期数据。

const CACHE = "koku-shell-v1";

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(["/", "/index.html", "/manifest.webmanifest"]))
      .then(() => self.skipWaiting())
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key))))
      .then(() => self.clients.claim())
  );
});

self.addEventListener("fetch", (event) => {
  const { request } = event;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (url.pathname.startsWith("/api/")) return; // 账本 API 不缓存。
  if (url.origin !== location.origin) return; // 只处理同源资源。

  event.respondWith(
    caches.match(request).then((cached) => {
      if (cached) return cached;
      return fetch(request).then((response) => {
        const isStatic = /\.(js|css|png|svg|ico|webmanifest|woff2?)$/.test(url.pathname);
        if (response.ok && (request.mode === "navigate" || isStatic)) {
          const copy = response.clone();
          caches.open(CACHE).then((cache) => cache.put(request, copy));
        }
        return response;
      });
    })
  );
});
