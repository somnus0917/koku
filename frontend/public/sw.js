// Koku 基础 Service Worker：缓存应用外壳与静态资源，离线时回退到缓存。
// API 请求始终走网络（不缓存账本数据），避免离线时展示过期数据。
//
// 版本策略：每次发布必须把 CACHE 版本号 +1（如 v1 → v2）。
// activate 时会清掉所有旧版本缓存，否则旧应用外壳会一直命中缓存优先策略，
// 让浏览器停留在上一个版本的 bundle 上——旧 bundle 渲染不了新数据（如
// deposit 类型流水）时会直接白屏。
//
// 资源策略：
// - 导航请求（页面外壳 index.html）：网络优先，发布后下一次访问即生效；
//   离线时回退到最近一次缓存的外壳。
// - 带内容哈希的静态资源（JS/CSS/图片等）：缓存优先，哈希即版本，永不脏读。
// - 其他资源：网络优先，成功后缓存。

const CACHE = "koku-shell-v2";

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
      .then((keys) =>
        Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key)))
      )
      .then(() => self.clients.claim())
  );
});

self.addEventListener("fetch", (event) => {
  const { request } = event;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (url.pathname.startsWith("/api/")) return; // 账本 API 不缓存。
  if (url.origin !== location.origin) return; // 只处理同源资源。

  const isNavigate = request.mode === "navigate";
  const isStatic = /\.(js|css|png|svg|ico|webmanifest|woff2?)$/.test(url.pathname);

  if (isNavigate) {
    // 页面外壳：网络优先（新发布立即生效），离线时回退缓存外壳。
    event.respondWith(
      fetch(request)
        .then((response) => {
          const copy = response.clone();
          caches.open(CACHE).then((cache) => cache.put(request, copy));
          return response;
        })
        .catch(() =>
          caches.match(request).then((cached) => cached || caches.match("/index.html"))
        )
    );
    return;
  }

  if (isStatic) {
    // 内容哈希静态资源：缓存优先（哈希变化即新 URL，不会读到旧内容）。
    event.respondWith(
      caches.match(request).then((cached) => {
        if (cached) return cached;
        return fetch(request).then((response) => {
          if (response.ok) {
            const copy = response.clone();
            caches.open(CACHE).then((cache) => cache.put(request, copy));
          }
          return response;
        });
      })
    );
    return;
  }

  // 其他资源：网络优先，成功后缓存。
  event.respondWith(
    fetch(request)
      .then((response) => {
        if (response.ok) {
          const copy = response.clone();
          caches.open(CACHE).then((cache) => cache.put(request, copy));
        }
        return response;
      })
      .catch(() => caches.match(request))
  );
});
