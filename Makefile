# Koku 本地开发 / 一键预览。
.PHONY: preview api web hash

## 默认目标：一键预览（后端 API + 前端 dev server，Ctrl+C 一起退出）
preview:
	./scripts/preview.sh

## 只启动后端 API（默认 http://127.0.0.1:8080）
api:
	./scripts/preview.sh --api

## 只启动前端 dev server（http://127.0.0.1:5173，/api 代理到 8080）
web:
	cd frontend && npm run dev

## 生成 bcrypt 密码哈希：make hash PASSWORD=你的密码（不传则用 koku-preview）
hash:
	cargo run --quiet --bin gen_hash -- $(PASSWORD)
