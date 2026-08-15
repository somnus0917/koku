# Koku

[![CI](https://github.com/somnus0917/koku/actions/workflows/ci.yml/badge.svg)](https://github.com/somnus0917/koku/actions/workflows/ci.yml)

Koku 是一个隐私优先、可私有部署且前后端分离的个人记账应用。设计借鉴了 [Sure](https://github.com/we-promise/sure) 克制、清晰的财务工作台体验，但采用独立的 Koku 视觉语言与 Rust 实现。浏览器通过 HTTPS API 将账本数据写入你自己的服务器，而不是仅保存在当前设备中。

## 功能

- 每个账户保持一个结算币种和一个共享余额，每笔交易可独立选择 CNY、USD 等原始币种
- 账户类型：零钱、储蓄、股票、信用（可负债）
- 收入、支出、同币种与跨币种转账
- 定期存款：储蓄转定期（自定义利率与期限），到期按实际持有天数结息并转回
- 报销：支出可标记待报销，支持部分报销；已报销金额不计入月度支出
- 借款：借入/借出任意账户（如从储蓄借出、还到零钱），未结应收/应付计入净资产
- SQLite 原子余额更新与账单撤销
- `rust_decimal` 精确货币计算，API 金额统一序列化为字符串
- 月度收支、净结余、分类占比与净资产统计
- 28 个开箱即用的收入/支出分类，并支持自定义补充
- 每个预设分类拥有独立图标、配色和头像，自定义分类自动生成稳定视觉样式
- 桌面 Sankey 与手机纵向流量卡片两套现金流视图
- 响应式桌面/移动界面、底部快捷导航、浅色/深色主题
- 本地 SQLite 持久化，首次启动自动生成演示账本
- 旧版 SQLite 数据启动时自动迁移（含 asset/liability → 零钱/信用），无需手工转换
- Docker Compose 生产部署、HTTPS 入口、访问认证与 GitHub Actions 自动发布
- 安全：登录失败限流（5 分钟窗口内 5 次即锁定该来源）、结构化日志（登录审计 + 请求级 tracing）、500 错误不回显内部细节

## 工程结构

```text
koku/
├── Cargo.toml          # Rust API 与领域核心
├── src/main.rs         # 进程入口与服务器启动
├── src/domain.rs       # 领域类型：账户/分类/交易枚举与 DTO
├── src/service.rs      # SQLite 持久化、记账业务、软撤销与迁移
├── src/api.rs          # REST API 处理器、鉴权中间件与路由
├── src/auth.rs         # 登录配置与会话 Cookie/令牌工具
├── src/config.rs       # 环境变量解析
├── src/demo.rs         # 控制台演示与演示账本种子
├── src/error.rs        # 统一错误类型与 HTTP 映射
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
KOKU_AUTH_USERNAME=somnus \
KOKU_AUTH_PASSWORD_HASH='$2b$12$替换为你自己的bcrypt哈希' \
KOKU_COOKIE_SECURE=false \
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

可通过环境变量覆盖后端配置。上面的 `KOKU_COOKIE_SECURE=false` 仅用于本机 HTTP 开发；生产环境保持默认值 `true`：

```bash
KOKU_PORT=9000 \
KOKU_DB_PATH=/path/to/koku.db \
KOKU_AUTH_USERNAME=somnus \
KOKU_AUTH_PASSWORD_HASH='$2b$12$替换为你自己的bcrypt哈希' \
KOKU_COOKIE_SECURE=false \
cargo run
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

这台腾讯云已经运行统一的 `edge-caddy` 并占用 `80/443`，因此 Koku 只启动两个独立容器：`web` 提供 React 静态文件并在内部代理 `/api`，`api` 运行 Rust 服务并独占 SQLite 写入。两个容器都不向宿主机公开端口；`web` 加入现有的外部 `proxy` 网络，由 `edge-caddy` 统一负责域名与 HTTPS，登录认证由 Koku API 自身完成。

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
- `KOKU_AUTH_USERNAME`：单用户登录名。
- `KOKU_SESSION_TTL_DAYS`：登录会话有效天数，范围为 1–365。
- `DEBIAN_MIRROR`：腾讯云建议使用 `http://mirrors.cloud.tencent.com`；Cargo 构建已固定使用 USTC 稀疏索引并启用缓存。

使用 Caddy 自带的 bcrypt 工具生成应用登录密码哈希，并单独保存在数据目录：

```bash
docker run --rm caddy:2-alpine caddy hash-password --plaintext '换成一个强密码' \
  > ~/koku/data/auth-password.hash
chmod 600 ~/koku/data/auth-password.hash
```

参考 [Caddy 站点模板](deploy/Caddyfile.example)，把替换好域名的站点块加入现有的 `~/caddy/Caddyfile`。随后验证并无中断重载现有入口：

```bash
docker exec edge-caddy caddy validate --config /etc/caddy/Caddyfile
docker exec edge-caddy caddy reload --config /etc/caddy/Caddyfile
```

该站点块代理到 `koku-web:8080`。登录页可以公开加载，但除健康检查和登录接口外，所有账本 API 都会验证服务器端会话。浏览器只保存 `HttpOnly`、`Secure`、`SameSite=Strict` Cookie，SQLite 只保存随机会话令牌的 SHA-256 摘要。

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

SQLite 数据位于 `KOKU_DATA_DIR`，默认是 `~/koku/data/koku.db`。浏览器会通过 HTTPS API 把账户与交易提交到这台服务器；“私有”表示数据不离开你的自托管服务器，并不表示只保存在访问设备中。容器重建不会删除此目录；不要把数据库、密码哈希或 `.env` 提交到 Git。

## 生产环境变量

| 变量 | 默认值 | 用途 |
| --- | --- | --- |
| `KOKU_HOST` | `127.0.0.1` | API 监听地址；容器内使用 `0.0.0.0` |
| `KOKU_PORT` | `8080` | API 监听端口 |
| `KOKU_DB_PATH` | `data/koku.db` | SQLite 文件路径 |
| `KOKU_SEED_DEMO` | `true` | 是否为空数据库生成演示账本；生产容器固定为 `false` |
| `KOKU_ALLOWED_ORIGIN` | 未设置 | 可选的单一跨域来源；同域生产部署无需开启 CORS |
| `KOKU_AUTH_USERNAME` | 必填 | 单用户登录名 |
| `KOKU_AUTH_PASSWORD_HASH` | 未设置 | bcrypt 密码哈希，适合本地运行；生产环境使用文件 |
| `KOKU_AUTH_PASSWORD_HASH_FILE` | 未设置 | bcrypt 哈希文件；生产容器固定为 `/app/data/auth-password.hash` |
| `KOKU_SESSION_TTL_DAYS` | `30` | 会话有效天数，范围 1–365 |
| `KOKU_COOKIE_SECURE` | `true` | 是否只允许 HTTPS 发送会话 Cookie；本地 HTTP 开发设为 `false` |
| `RUST_LOG` | `auth=info,koku=info,tower_http=info` | tracing 日志级别；如 `RUST_LOG=debug` 可看到请求级日志 |

## REST API

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `GET` | `/api/health` | 健康检查 |
| `POST` | `/api/auth/login` | 校验用户名密码并创建安全会话 |
| `GET` | `/api/auth/session` | 查询当前登录用户 |
| `POST` | `/api/auth/logout` | 作废当前服务器会话并清除 Cookie |
| `GET/POST` | `/api/accounts` | 查询或创建账户 |
| `PATCH` | `/api/accounts/{id}` | 编辑账户（名称/类型/币种；有交易历史时不可改币种） |
| `POST` | `/api/accounts/{id}/adjust-balance` | 余额调整（带符号增量，生成可追溯的调整流水） |
| `GET/POST` | `/api/categories` | 查询或创建分类 |
| `DELETE` | `/api/categories/{id}` | 删除分类；历史账单和统计保留原分类 |
| `GET/POST` | `/api/transactions` | 查询或记录收入/支出；查询支持 `?limit=&offset=` 分页（默认 `limit=500`，上限 1000） |
| `POST` | `/api/transfers` | 原子账户转账 |
| `DELETE` | `/api/transactions/{id}` | 撤销交易并恢复余额 |
| `POST/DELETE` | `/api/transactions/{id}/reimbursable` | 标记/取消"待报销"（已发生报销的支出不可取消） |
| `POST` | `/api/reimbursements` | 报销支出（支持部分报销，生成关联收入流水；撤销支出会级联撤销报销收入） |
| `POST` | `/api/deposits` | 储蓄转定期（利率 + 期限） |
| `POST` | `/api/deposits/{id}/settle` | 结清定期：按持有天数计息并把本息转回 |
| `GET/POST` | `/api/loans` | 查询或创建借出/借入 |
| `POST` | `/api/loans/{id}/repay` | 还款（任意账户进出，归零自动结清） |
| `GET` | `/api/summary/monthly` | 按年月与币种查询收支；所有币种的流水统一按汇率折算到该币种 |
| `GET` | `/api/summary/cash-flow` | 查询收入来源、支出去向和结余现金流（多币种按汇率折算） |
| `GET` | `/api/summary/balance` | 按币种查询资产、负债与净值（所有币种账户与未结借款按汇率折算） |
| `GET` | `/api/rates?from=&to=` | 汇率提示：1 from = rate to（Frankfurter/ECB 参考中间价，本地缓存，源不可达时回退旧缓存） |

`DELETE` 使用审计友好的软撤销语义，不物理删除交易记录。

创建收入/支出时通过 `currency` 指定原始交易币种。币种与账户结算币种不同时，同时提交 `settled_amount` 作为实际计入共享余额的金额，例如 `$10.00` 消费可按 `¥72.00` 入账。流水和月度收支按原始币种统计，账户余额始终使用账户结算币种。
