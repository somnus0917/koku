#!/usr/bin/env bash
# 启动一次性后端与 Vite，运行 Playwright 全栈关键流程，并确保退出时清理。

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
E2E_DIR="$(mktemp -d)"
API_LOG="$E2E_DIR/api.log"
WEB_LOG="$E2E_DIR/web.log"
API_PID=""
WEB_PID=""

cleanup() {
  if [[ -n "$WEB_PID" ]]; then
    kill -- "-$WEB_PID" 2>/dev/null || kill "$WEB_PID" 2>/dev/null || true
  fi
  if [[ -n "$API_PID" ]]; then
    kill -- "-$API_PID" 2>/dev/null || kill "$API_PID" 2>/dev/null || true
  fi
  rm -rf "$E2E_DIR"
}
trap cleanup EXIT INT TERM

cd "$ROOT"
PASSWORD="koku-e2e-password"
PASSWORD_HASH="$(cargo run --quiet --bin gen_hash -- "$PASSWORD")"

setsid env \
  KOKU_DB_PATH="$E2E_DIR/koku.db" \
  KOKU_AUTH_EMAIL="e2e@example.com" \
  KOKU_AUTH_PASSWORD_HASH="$PASSWORD_HASH" \
  KOKU_COOKIE_SECURE="false" \
  KOKU_SEED_DEMO="true" \
  KOKU_QUOTE_AUTO_REFRESH="false" \
  KOKU_PORT="8080" \
  cargo run --quiet --bin koku >"$API_LOG" 2>&1 &
API_PID=$!

setsid npm --prefix frontend run dev -- --host 127.0.0.1 --port 5173 >"$WEB_LOG" 2>&1 &
WEB_PID=$!

for _ in $(seq 1 90); do
  if curl -fsS http://127.0.0.1:8080/api/health >/dev/null 2>&1 \
    && curl -fsS http://127.0.0.1:5173/ >/dev/null 2>&1; then
    cd frontend
    npx playwright test
    exit
  fi
  if ! kill -0 "$API_PID" 2>/dev/null || ! kill -0 "$WEB_PID" 2>/dev/null; then
    break
  fi
  sleep 1
done

echo "E2E services did not become ready" >&2
echo "API log:" >&2
tail -n 80 "$API_LOG" >&2
echo "Web log:" >&2
tail -n 80 "$WEB_LOG" >&2
exit 1
