#!/usr/bin/env bash
# Koku 本地一键预览：启动后端 API（自动带引导登录凭据）与前端 dev server。
#
# 用法：
#   ./scripts/preview.sh          # 后端 + 前端（Ctrl+C 一起退出）
#   ./scripts/preview.sh --api    # 只启动后端 API
#   make preview                  # 等价于 ./scripts/preview.sh
#
# 环境变量（均可选）：
#   KOKU_AUTH_USERNAME      登录名（默认 somnus）
#   KOKU_PREVIEW_PASSWORD   预览密码（默认 koku-preview；首次运行会打印，哈希缓存于 .preview/）
#   KOKU_AUTH_PASSWORD_HASH 若已设置则直接使用该 bcrypt 哈希，否则自动生成
#   KOKU_PORT               后端端口（默认 8080）
#   KOKU_COOKIE_SECURE      本机 HTTP 开发默认 false（生产保持 true）

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PORT="${KOKU_PORT:-8080}"
API_URL="http://127.0.0.1:${PORT}"
WEB_URL="http://127.0.0.1:5173"
USERNAME="${KOKU_AUTH_USERNAME:-somnus}"
PASSWORD="${KOKU_PREVIEW_PASSWORD:-koku-preview}"

# 引导凭据：优先用户提供的哈希 → 缓存哈希 → 现场生成并缓存。
ensure_auth_hash() {
  if [[ -n "${KOKU_AUTH_PASSWORD_HASH:-}" ]]; then
    printf '%s' "$KOKU_AUTH_PASSWORD_HASH"
    return
  fi
  local dir="$ROOT/.preview"
  local hash_file="$dir/auth.hash"
  local pass_file="$dir/auth.pass"
  if [[ -f "$hash_file" ]] && [[ -f "$pass_file" ]] && [[ "$(cat "$pass_file")" == "$PASSWORD" ]]; then
    cat "$hash_file"
    return
  fi
  mkdir -p "$dir"
  echo "==> 首次运行：生成预览登录凭据  $USERNAME / $PASSWORD（哈希已缓存，后续不再重复生成）"
  local hash
  hash="$(cargo run --quiet --bin gen_hash -- "$PASSWORD")"
  printf '%s' "$hash" > "$hash_file"
  printf '%s' "$PASSWORD" > "$pass_file"
  printf '%s' "$hash"
}

start_api() {
  local hash
  hash="$(ensure_auth_hash)"
  echo "==> 启动后端 API  $API_URL  (登录名 $USERNAME)"
  KOKU_AUTH_USERNAME="$USERNAME" \
  KOKU_AUTH_PASSWORD_HASH="$hash" \
  KOKU_COOKIE_SECURE="${KOKU_COOKIE_SECURE:-false}" \
  cargo run --bin koku
}

if [[ "${1:-}" == "--api" ]]; then
  start_api
  exit 0
fi

# 后端放入独立进程组后台启动，脚本退出（Ctrl+C / 结束）时整体回收。
start_api &
API_PID=$!
cleanup() {
  kill -- "-$API_PID" 2>/dev/null || kill "$API_PID" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

echo "==> 等待后端就绪 ..."
ready=0
for _ in $(seq 1 60); do
  if curl -fsS "$API_URL/api/health" >/dev/null 2>&1; then
    ready=1
    break
  fi
  if ! kill -0 "$API_PID" 2>/dev/null; then
    echo "!! 后端进程已退出，请查看上方错误" >&2
    exit 1
  fi
  sleep 1
done
if [[ "$ready" != 1 ]]; then
  echo "!! 后端 ${PORT} 端口 60 秒内未就绪，请检查上方日志" >&2
  exit 1
fi

echo "==> 启动前端  $WEB_URL  （登录：$USERNAME / $PASSWORD）"
cd frontend
npm run dev
