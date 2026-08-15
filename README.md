# Koku

[![CI](https://github.com/somnus0917/koku/actions/workflows/ci.yml/badge.svg)](https://github.com/somnus0917/koku/actions/workflows/ci.yml)

Koku 是一个本地优先、前后端分离的个人记账 MVP。设计借鉴了 [Sure](https://github.com/we-promise/sure) 克制、清晰的财务工作台体验，但采用独立的 Koku 视觉语言与 Rust 实现。

## 功能

- 每个账户保持一个结算币种和一个共享余额，每笔交易可独立选择 CNY、USD 等原始币种
- 收入、支出、同币种与跨币种转账
- SQLite 原子余额更新与账单撤销
- `rust_decimal` 精确货币计算，API 金额统一序列化为字符串
- 月度收支、净结余、分类占比与净资产统计
- 28 个开箱即用的收入/支出分类，并支持自定义补充
- 每个预设分类拥有独立图标、配色和头像，自定义分类自动生成稳定视觉样式
- 桌面 Sankey 与手机纵向流量卡片两套现金流视图
- 响应式桌面/移动界面、底部快捷导航、浅色/深色主题
- 本地 SQLite 持久化，首次启动自动生成演示账本
- 旧版单币种 SQLite 数据启动时自动迁移，无需手工转换
- Docker Compose 生产部署、HTTPS 入口、访问认证与 GitHub Actions 自动发布

## 工程结构

```text
koku/
├── Cargo.toml          # Rust API 与领域核心
├── src/main.rs         # SQLite、Service、REST API、CLI Demo 与测试
├── data/koku.db        # 首次运行自动创建，不纳入版本控制
└── frontend/
    ├── src/App.tsx     # 页面、业务交互和组件
    ├── src/api.ts      # 独立 API 客户端
    ├── src/styles.css  # Koku 视觉系统与响应式布局
    └── vite.config.ts  # 开发代理 /api -> 127.0.0.1:8080
```

## 本地运行

终端一，启动后端：

```bash
cargo run
```

后端默认监听 `http://127.0.0.1:8080`，数据写入 `data/koku.db`。

终端二，启动前端：

```bash
cd frontend
npm install
npm run dev
```

浏览器打开 `http://127.0.0.1:5173`。

可通过环境变量覆盖后端配置：

```bash
KOKU_PORT=9000 KOKU_DB_PATH=/path/to/koku.db cargo run
```

若前端不使用 Vite 开发代理，可创建 `frontend/.env.local`：

```dotenv
VITE_API_BASE_URL=http://127.0.0.1:8080
```

## 验证

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cd frontend && npm run build
```

同样的检查已配置在 [GitHub Actions CI](.github/workflows/ci.yml)，会在推送到 `main` 或创建 Pull Request 时自动运行。

保留原始控制台演示入口：

```bash
cargo run -- --demo
```

## 腾讯云自动部署

这台腾讯云已经运行统一的 `edge-caddy` 并占用 `80/443`，因此 Koku 只启动两个独立容器：`web` 提供 React 静态文件并在内部代理 `/api`，`api` 运行 Rust 服务并独占 SQLite 写入。两个容器都不向宿主机公开端口；`web` 加入现有的外部 `proxy` 网络，由 `edge-caddy` 统一负责域名、HTTPS 与 Basic Auth。

### 1. 初始化服务器

服务器需要安装 Docker Engine 和 Docker Compose v2。当前镜像由 GitHub 的 x86-64 Runner 构建，因此腾讯云实例应使用 x86-64；ARM 实例需要在工作流中额外配置多架构构建。

为部署用户创建目录，并确认服务器上已经存在共享 `proxy` 网络：

```bash
mkdir -p ~/koku/data/backups ~/koku/deploy
docker network inspect proxy >/dev/null
```

将示例配置传到服务器并改名：

```bash
scp .env.production.example YOUR_USER@YOUR_SERVER:koku/.env
```

编辑 `~/koku/.env`：

- `KOKU_DOMAIN`：已解析到这台 CVM 的域名。
- `KOKU_RUNTIME_UID/GID`：分别使用服务器上的 `id -u` 和 `id -g`；当前 `ubuntu` 用户均为 `1000`。

在服务器生成 Basic Auth 密码哈希：

```bash
docker run --rm caddy:2.10-alpine caddy hash-password --plaintext '换成一个强密码'
```

参考 [Caddy 站点模板](deploy/Caddyfile.example)，把替换好域名、用户名和密码哈希的站点块加入现有的 `~/caddy/Caddyfile`。随后验证并无中断重载现有入口：

```bash
docker exec edge-caddy caddy validate --config /etc/caddy/Caddyfile
docker exec edge-caddy caddy reload --config /etc/caddy/Caddyfile
```

该站点块代理到 `koku-web:8080`。认证必须放在现有 Caddy 上，才能同时保护页面和所有写入型 `/api` 接口。

生产部署采用与 `luopanhacker` 相同的受限 SSH 模式。CI 成功后，Actions 只向服务器发送已经验证过的 40 位 Git commit SHA；服务器下载该不可变源码归档并复用 Docker 层缓存完成构建，不通过 SCP 搬运源码或镜像，也不需要保存 GitHub PAT。

Actions 使用单独的无密码部署密钥。该公钥在服务器上通过 `restrict,command="/usr/local/sbin/koku-ssh-gateway"` 强制进入命令网关，网关只接受 `deploy <40 位 SHA>`，再通过最小化 sudo 规则以 `ubuntu` 身份执行部署脚本。可以复用已加入腾讯云登录告警白名单的 `luopan-deploy` 用户，但必须为 Koku 使用独立密钥和独立强制命令。

腾讯云安全组只需向公网开放 TCP `80/443` 和 UDP `443`；SSH 端口应限制为自己的可信 IP。域名的 A/AAAA 记录必须先指向服务器，Caddy 才能自动申请证书。

### 2. 配置 GitHub

在仓库的 `Settings → Environments` 新建 `production` Environment，并限制只有 `main` 可以部署。添加变量：

- `KOKU_DOMAIN`：生产域名，用于 Actions 部署页面链接。

添加 Environment Secrets：

- `SERVER_HOST`：腾讯云公网 IP 或主机名。
- `SERVER_PORT`：SSH 端口，通常为 `22`。
- `SERVER_USER`：有权运行 Docker 的非 root 部署用户。
- `SERVER_SSH_KEY`：该用户的 Ed25519 私钥。
- `SERVER_KNOWN_HOSTS`：预先核验过指纹的 SSH `known_hosts` 条目。

不要在 Actions 中临时信任未经核验的 `ssh-keyscan` 输出。首次连接服务器时应通过腾讯云控制台核验主机指纹，再保存 `known_hosts` 内容。

### 3. 自动发布过程

推送到 `main` 后，[CI 工作流](.github/workflows/ci.yml) 与 [生产部署工作流](.github/workflows/deploy.yml) 会依次：

1. 运行 Rust 测试、格式检查、Clippy、前端构建以及部署配置检查。
2. 部署工作流确认通过 CI 的 SHA 仍然是 `main` 最新提交，过期结果会被跳过。
3. Actions 使用受限密钥发送唯一允许的 `deploy <SHA>` 命令。
4. 服务器下载该 SHA 的源码归档，并同步到 `~/koku`，保留 `.env`、SQLite 与发布状态文件。
5. 在线备份现有 SQLite，利用服务器 Docker 缓存构建两个镜像。
6. 启动新镜像并等待健康检查；失败时恢复上一组镜像。

也可以在 GitHub Actions 页面通过 `workflow_dispatch` 手动重新部署 `main`。

服务器排查命令：

```bash
cd ~/koku
docker compose --env-file .env --env-file .release.env -f compose.production.yml ps
docker compose --env-file .env --env-file .release.env -f compose.production.yml logs --tail=200
```

SQLite 数据位于 `KOKU_DATA_DIR`，默认是 `~/koku/data/koku.db`。容器重建不会删除此目录；不要把数据库或 `.env` 提交到 Git。

## 生产环境变量

| 变量 | 默认值 | 用途 |
| --- | --- | --- |
| `KOKU_HOST` | `127.0.0.1` | API 监听地址；容器内使用 `0.0.0.0` |
| `KOKU_PORT` | `8080` | API 监听端口 |
| `KOKU_DB_PATH` | `data/koku.db` | SQLite 文件路径 |
| `KOKU_SEED_DEMO` | `true` | 是否为空数据库生成演示账本；生产容器固定为 `false` |
| `KOKU_ALLOWED_ORIGIN` | 未设置 | 可选的单一跨域来源；同域生产部署无需开启 CORS |

## REST API

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `GET` | `/api/health` | 健康检查 |
| `GET/POST` | `/api/accounts` | 查询或创建账户 |
| `GET/POST` | `/api/categories` | 查询或创建分类 |
| `GET/POST` | `/api/transactions` | 查询或记录收入/支出 |
| `POST` | `/api/transfers` | 原子账户转账 |
| `DELETE` | `/api/transactions/{id}` | 撤销交易并恢复余额 |
| `GET` | `/api/summary/monthly` | 按年月与币种查询收支 |
| `GET` | `/api/summary/cash-flow` | 查询收入来源、支出去向和结余现金流 |
| `GET` | `/api/summary/balance` | 按币种查询资产、负债与净值 |

`DELETE` 使用审计友好的软撤销语义，不物理删除交易记录。

创建收入/支出时通过 `currency` 指定原始交易币种。币种与账户结算币种不同时，同时提交 `settled_amount` 作为实际计入共享余额的金额，例如 `$10.00` 消费可按 `¥72.00` 入账。流水和月度收支按原始币种统计，账户余额始终使用账户结算币种。
