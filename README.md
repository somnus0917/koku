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
- 月度收支、净结余、分类占比与净资产统计，以及近 12 个月收支趋势图
- 28 个开箱即用的收入/支出分类，并支持自定义补充
- 每个预设分类拥有独立图标、配色和头像，自定义分类自动生成稳定视觉样式
- 桌面 Sankey 与手机纵向流量卡片两套现金流视图
- 交易列表分页 + 按月筛选、全文搜索与标签筛选
- 月度预算：按分类设置上限，超支红色进度条
- 周期交易：房租/订阅等固定收支按月/周自动生成（请求驱动，无后台任务）
- CSV 导出：全部或按月的交易流水，浏览器直接下载
- 标签：跨类目聚合（多对多），表单自由输入 + 列表筛选
- 股票持仓：买入/卖出（含手续费、摊薄成本）、市价更新，持仓市值计入净资产；按代码识别 A 股/科创板/港股/美股，美股优先 Nasdaq、其余市场优先 Stooq，均可回退 Yahoo Finance，并显示价格来源与日期
- 账户对账：以对账单余额为目标余额，完成时自动生成可审计的调整流水（撤销即回滚）
- 信用卡账单：信用账户可设账单日/还款日（1~31，无对应日期自动落到月末），账户页展示额度/已用/可用、本期已出账与未出账金额、下次账单与还款日及近期账单历史；消费 = Credit 账户上的支出，还款 = 储蓄 → 信用卡的转账（**还款不是支出**，不会重复统计）；每个结束的账单周期在首次读取时固化为不可变快照，未出账部分按 `occurred_at` 动态计算；还款/贷项按 FIFO 冲抵最早账单（可解释近似，不追踪「某次还款具体还哪一期」）；金额全程 `Decimal` 精确求和，不使用浮点
- 到期提醒：API 返回未来 N 天内到期（含逾期）的定存、借款与未还信用卡账单；可选 SMTP 每日邮件摘要
- 报销附件：交易可挂小票/发票图片（存 SQLite BLOB）
- 到期提醒：Dashboard 顶部提示已到期未结的定存与借款
- 应用内改密码（修改后旧会话全部失效）
- TOTP 二步验证：登录分两步，应用内自助开启/关闭（基于 `totp-rs`）
- 数据库备份/恢复：管理员一键备份（共享库 + 全部用户账本打包 zip）、下载、恢复；可选定时备份；可选自动上传 Cloudflare R2 实现异地冗余（含从 R2 恢复）
- CSV 导入：支持 Koku 收支交易 CSV 再导入（保留分类、Payee 与原始描述等交易元数据，需自行选择目标账户）、通用银行流水 CSV（中文/英文列名别名）、QIF、OFX（SGML/XML），先预览后确认、逐行去重与错误汇总，并可整批软撤销。注意：CSV 是数据交换/编辑/再导入格式，不是完整账本备份——账户、转账与复杂交易不会自动重建，完整备份/恢复请使用数据库 ZIP backup
- 商户/收款方 Payee：交易可关联商户，输入时自动补全，列表展示并支持搜索
- 自动学习分类（本地统计，非 AI）：根据用户确认的「原始描述 → 商户」映射与「商户 → 分类」历史频率自动识别商户并预测分类；高置信度自动分类、中置信度给出可一键采纳的分类建议；人工纠正会即时更新统计。学习数据只存在用户自己的 SQLite 账本中，可在个人设置中清除
- 年度汇总与滚动平均：按年统计逐月收支与分类明细，最近 N 个月收支的滚动均值视图
- 通用 API 限流：除健康检查外所有 `/api` 请求按客户端限流（默认 300 次/分钟）
- 深色/浅色/跟随系统三种主题，本地持久化并实时跟随系统偏好
- 多语言界面：中文（默认）/ English，顶栏与登录页可一键切换（金额与日期格式随语言变化）
- PWA：可添加到主屏幕，离线缓存应用外壳（账本 API 仍走网络）
- 快速记账：记住上次使用的账户/分类，打开即预填
- 响应式桌面/移动界面、底部快捷导航、浅色/深色主题
- 本地 SQLite 持久化，首次启动自动生成演示账本
- 旧版 SQLite 数据启动时自动迁移（含 asset/liability → 零钱/信用），无需手工转换
- Docker Compose 生产部署、HTTPS 入口、访问认证与 GitHub Actions 自动发布
- 安全：登录失败限流（5 分钟窗口内 5 次即锁定该来源）、可选 TOTP 二步验证、通用 API 限流（默认 300 次/分钟/客户端）、结构化日志（登录审计 + 请求级 tracing）、500 错误不回显内部细节

## 工程结构

```text
koku/
├── Cargo.toml          # Rust API 与领域核心
├── src/main.rs         # 进程入口、定时备份与 SMTP 提醒后台任务
├── src/domain.rs       # 领域类型：账户/分类/交易/商户/对账/提醒枚举与 DTO
├── src/service/        # SQLite 持久化与记账业务（按领域拆分子模块）
│   ├── mod.rs          # BookkeepingService 核心、共享 helper 与模块声明
│   ├── schema.rs       # 建表/索引初始化（幂等）
│   ├── migrations.rs   # 旧库兼容迁移（补列、整表重建）
│   ├── transactions.rs # 收支/转账/软撤销/流水查询
│   ├── payees.rs       # 商户归一化、别名学习、分类统计与预测
│   ├── import.rs       # 批量导入写入与学习统计
│   └── …               # accounts/budgets/loans/recurring/summaries/users 等
├── src/api/            # REST 处理器、鉴权中间件与路由（按领域拆分子模块）
│   ├── mod.rs          # 路由装配与全局中间件（鉴权/CORS/限流/Trace）
│   ├── state.rs        # AppState / AuthenticatedUser / 账本锁
│   ├── auth.rs         # 登录/TOTP/会话/密码
│   ├── transactions.rs # 流水 CRUD
│   ├── payees.rs       # 商户搜索与学习数据清理
│   └── …               # accounts/categories/deposits/loans/budgets 等
├── src/auth.rs         # 登录配置与会话 Cookie/令牌工具
├── src/totp.rs         # TOTP 密钥生成/校验/otpauth URI
├── src/backup.rs       # 备份/恢复：VACUUM INTO 快照 + zip 打包
├── src/importer.rs     # CSV/QIF/OFX 账单解析
├── src/quotes.rs       # 多源行情客户端（Stooq → Yahoo Finance）
├── src/mailer.rs       # 可选 SMTP 邮件发送
├── src/ratelimit.rs    # 通用 API 限流（固定窗口按客户端）
├── src/config.rs       # 环境变量解析
├── src/demo.rs         # 控制台演示与演示账本种子
├── src/error.rs        # 统一错误类型与 HTTP 映射
├── data/koku.db        # 首次运行自动创建，不纳入版本控制
└── frontend/
    ├── src/app/        # 应用外壳：App 会话路由 / LedgerApp 页面编排 / Sidebar / Topbar
    ├── src/api/        # 独立 API 客户端（按领域拆分子模块 + client 封装）
    ├── src/features/   # 按业务领域的页面与弹窗（accounts/transactions/payees/settings…）
    ├── src/i18n/       # 多语言资源（locales/zh.ts、locales/en.ts）
    ├── src/components/ # 共享 UI 组件（ModalShell / RateHint / PageTitle…）
    ├── src/theme.ts    # 浅色/深色/跟随系统主题 hook
    ├── src/styles.css  # Koku 视觉系统与响应式布局
    └── vite.config.ts  # 开发代理 /api -> 127.0.0.1:8080
```

## 本地运行

终端一，启动后端：

```bash
KOKU_AUTH_EMAIL=somnus@example.com \
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
KOKU_AUTH_EMAIL=somnus@example.com \
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
cargo audit
cd frontend
npm audit --omit=dev
npm run build
```

`cargo audit` 需先安装 `cargo-audit`（`cargo install cargo-audit --locked`）。同样的检查已配置在 [GitHub Actions CI](.github/workflows/ci.yml)，会在推送到 `main` 或创建 Pull Request 时自动运行；RustSec 扫描完整的 Cargo 锁定依赖树，npm 扫描通过 `--omit=dev` 仅检查前端生产依赖。`.cargo/audit.toml` 只豁免未启用的 `rust_decimal/rkyv` 可选依赖告警，并在文件内记录原因。

保留原始控制台演示入口：

```bash
cargo run -- --demo
```

## 腾讯云自动部署

这台腾讯云已经运行统一的 `edge-caddy` 并占用 `80/443`，因此 Koku 只启动两个独立容器：`web` 提供 React 静态文件并在内部代理 `/api`，`api` 运行 Rust 服务并独占 SQLite 写入。两个容器都不向宿主机公开端口；`web` 加入现有的外部 `proxy` 网络，由 `edge-caddy` 统一负责域名与 HTTPS，登录认证由 Koku API 自身完成。

> **出站网络要求**：汇率提示功能（跨币种折算/预填）需要 API 容器能访问两个公开汇率源——
> `api.frankfurter.app`（主源）和 `cdn.jsdelivr.net`（备用源）。仅拉取公开汇率，不涉及账本数据。
> 若服务器出口有防火墙/安全组限制，请放行这两个域名；否则汇率拉取会一直失败，展示层只能
> 回退到本地缓存（首次使用前为空，跨币种折算会报缺汇率错误）。

### 1. 初始化服务器

服务器需要安装 Docker Engine 和 Docker Compose v2。API 与 Web 发布镜像均提供 `linux/amd64` 和 `linux/arm64`，腾讯云的 x86-64 与 ARM 实例都可直接拉取对应架构。

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

- `KOKU_DOMAIN`：已解析到这台 CVM 的域名；本部署为 `koku.somnus.wiki`。
- `KOKU_RUNTIME_UID/GID`：分别使用服务器上的 `id -u` 和 `id -g`；当前 `ubuntu` 用户均为 `1000`。
- `KOKU_AUTH_EMAIL` / `KOKU_AUTH_PASSWORD_HASH`：多用户引导凭据。首次启动时以该邮箱+密码创建 **管理员** 账号（应用内改过密码则优先用持久化的哈希）；此后登录全部走 `users` 表，这两个变量只影响全新初始化。`KOKU_AUTH_USERNAME` 仅为旧配置兼容别名。
- 多用户模型：每个用户拥有**完全独立的账本**（账户/分类/交易/标签/预算/借款/持仓/定期/小票等全部隔离），数据存放在 `data/ledgers/ledger-<id>.db`；共享库 `data/koku.db` 只保存用户与会话。**不开放注册**，新用户只能由管理员在「用户」页以邮箱创建；管理员可重置密码、启用/停用（立即作废其会话）、删除用户（连带其账本文件）。
- `KOKU_SESSION_TTL_DAYS`：登录会话有效天数，范围为 1–365。
- `DEBIAN_MIRROR`：腾讯云建议使用 `http://mirrors.cloud.tencent.com`；Cargo 构建已固定使用 USTC 稀疏索引并启用缓存。

使用 Caddy 自带的 bcrypt 工具生成应用登录密码哈希，并单独保存在数据目录：

```bash
docker run --rm caddy:2-alpine caddy hash-password --plaintext '换成一个强密码' \
  > ~/koku/data/auth-password.hash
chmod 600 ~/koku/data/auth-password.hash
```

将 [Caddy 站点块](deploy/Caddyfile.example) 加入现有的 `~/caddy/Caddyfile`（模板已配置 `koku.somnus.wiki`）。随后验证并无中断重载现有入口：

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

### GHCR 多架构镜像

发布 GitHub Release 时，[容器发布工作流](.github/workflows/release.yml) 使用 Buildx/QEMU 构建 API 与 Web 的 `linux/amd64,linux/arm64` manifest，并通过 `GITHUB_TOKEN` 推送到 GHCR：

```bash
docker pull ghcr.io/somnus0917/koku-api:<release-tag>
docker pull ghcr.io/somnus0917/koku-web:<release-tag>
```

非预发布 Release 同时更新 `latest` 标签，每次构建也会产生不可变的 `sha-<short-sha>` 标签。工作流支持手动运行；手动运行只发布 SHA 标签。现有受限 SSH 自动部署流程仍在目标服务器从已验证源码构建，不受镜像发布流程影响。

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
| `KOKU_AUTH_EMAIL` | 必填 | 首次启动时创建管理员的登录邮箱；`KOKU_AUTH_USERNAME` 是兼容旧配置的别名 |
| `KOKU_AUTH_PASSWORD_HASH` | 未设置 | bcrypt 密码哈希，适合本地运行；生产环境使用文件 |
| `KOKU_AUTH_PASSWORD_HASH_FILE` | 未设置 | bcrypt 哈希文件；生产容器固定为 `/app/data/auth-password.hash` |
| `KOKU_SESSION_TTL_DAYS` | `30` | 会话有效天数，范围 1–365 |
| `KOKU_COOKIE_SECURE` | `true` | 是否只允许 HTTPS 发送会话 Cookie；本地 HTTP 开发设为 `false` |
| `KOKU_RATE_LIMIT_PER_MINUTE` | `300` | 通用 API 限流：每客户端每分钟请求上限；`0` 关闭 |
| `KOKU_BACKUP_INTERVAL_HOURS` | `24` | 定时备份间隔（小时）；`0` 关闭（仅管理员手动触发） |
| `KOKU_JOBS_INTERVAL_MINUTES` | `60` | 服务端周期交易、预算结转与收盘后行情刷新检查间隔（分钟，1–1440） |
| `KOKU_BACKUP_KEEP` | `14` | 保留最近多少份备份，超出的自动清理 |
| `KOKU_QUOTE_TTL_HOURS` | `24` | 持仓市价缓存有效期（小时），超过视为过期并在刷新时重新拉取 |
| `KOKU_QUOTE_AUTO_REFRESH` | `true` | 是否在各识别市场收盘后自动刷新当日未更新的持仓行情 |
| `KOKU_SMTP_HOST` | 未设置 | SMTP 服务器（设置后启用到期提醒邮件；不设置则仅应用内提醒） |
| `KOKU_SMTP_PORT` | `587` | SMTP 端口 |
| `KOKU_SMTP_TLS` | `starttls` | SMTP 加密方式：`starttls` / `implicit`（465）/ `none` |
| `KOKU_SMTP_USERNAME` / `KOKU_SMTP_PASSWORD` | 未设置 | SMTP 认证（无认证服务器可留空） |
| `KOKU_SMTP_FROM` | 必填（启用时） | 发件人邮箱；收件人始终为每个启用用户的登录邮箱 |
| `KOKU_SMTP_INTERVAL_HOURS` | `24` | 到期提醒邮件发送间隔（小时） |
| `KOKU_R2_ACCOUNT_ID` | 未设置 | Cloudflare R2 账户 ID（设置后启用异地备份上传；需同时配置下面四项） |
| `KOKU_R2_ACCESS_KEY_ID` / `KOKU_R2_SECRET_ACCESS_KEY` | 未设置 | R2 API 令牌的 S3 凭据（需授予「对象读写」权限） |
| `KOKU_R2_BUCKET` / `KOKU_R2_PREFIX` | `backups` / `koku` | R2 桶名与对象前缀 |
| `RUST_LOG` | `auth=info,koku=info,tower_http=info` | tracing 日志级别；如 `RUST_LOG=debug` 可看到请求级日志 |

## REST API

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `GET` | `/api/health` | 健康检查 |
| `POST` | `/api/auth/login` | 校验邮箱密码并创建安全会话；已启用 TOTP 时返回 `totp_required` 进入第二步 |
| `POST` | `/api/auth/totp` | TOTP 第二步：校验 6 位动态码并创建会话（一次性令牌 5 分钟有效） |
| `GET` | `/api/auth/session` | 查询当前登录用户 |
| `POST` | `/api/auth/logout` | 作废当前服务器会话并清除 Cookie |
| `POST` | `/api/auth/password` | 应用内改密码（校验旧密码，作废该用户全部会话） |
| `POST` | `/api/auth/totp/setup` | （本人）TOTP 设置第一步：校验当前密码后生成新密钥（暂存，未启用） |
| `POST` | `/api/auth/totp/enable` | （本人）用动态码确认后启用 TOTP |
| `POST` | `/api/auth/totp/disable` | （本人）用动态码确认后关闭 TOTP |
| `GET/POST` | `/api/users` | （管理员）用户列表 / 创建成员用户 |
| `POST` | `/api/users/{id}/password` | （管理员）重置某用户密码（作废其会话） |
| `POST` | `/api/users/{id}/enabled` | （管理员）启用/停用用户（停用立即作废其会话） |
| `DELETE` | `/api/users/{id}` | （管理员）删除用户（连带其独立账本，不可恢复） |
| `GET/POST` | `/api/accounts` | 查询或创建账户 |
| `PATCH` | `/api/accounts/{id}` | 编辑账户（名称/类型/币种；有交易历史时不可改币种） |
| `GET` | `/api/accounts/{id}/credit-card-summary` | 信用卡额度、已用额度、已出账与未出账摘要 |
| `GET` | `/api/accounts/{id}/credit-card-statements` | 信用卡已固化账单及按 FIFO 计算的待还金额 |
| `POST` | `/api/accounts/{id}/adjust-balance` | 余额调整（带符号增量，生成可追溯的调整流水） |
| `GET/POST` | `/api/categories` | 查询或创建分类 |
| `DELETE` | `/api/categories/{id}` | 删除分类；历史账单和统计保留原分类 |
| `GET/POST` | `/api/transactions` | 查询或记录收入/支出；查询支持 `?limit=&offset=` 分页（默认 `limit=500`，上限 1000），可加 `?year=&month=`、`search=`、`kind=`、`tags=` 在服务端筛选；记录时可带 `tag_names` |
| `GET` | `/api/transactions/export` | 导出交易为 CSV（可选 `?year=&month=`），触发浏览器下载 |
| `POST` | `/api/transactions/import` | 批量导入流水（multipart：`file`/`format`(auto,csv,qif,ofx)/`account_id`/`category_id`/`currency`），逐行去重并返回错误汇总 |
| `POST` | `/api/transactions/import/preview` | 解析导入文件并返回行数、收支统计、问题与样例，不写入账本 |
| `POST` | `/api/transactions/import/{batch_id}/undo` | 整批软撤销一次导入产生的流水 |
| `POST` | `/api/transfers` | 原子账户转账 |
| `POST` | `/api/transactions/{id}/void` | 撤销交易并恢复余额（软删除） |
| `POST` | `/api/transactions/{id}/restore` | 撤销删除：恢复已撤销的流水（余额与报销状态一并恢复） |
| `DELETE` | `/api/transactions/{id}` | 永久删除已撤销的流水（连带小票/标签/报销记录，不可恢复） |
| `PATCH` | `/api/transactions/{id}` | 编辑收入/支出（备注/时间/分类/金额/账户/结算额/标签，余额原子联动；已撤销、转账/借款、已报销的流水有编辑限制） |
| `POST/DELETE` | `/api/transactions/{id}/reimbursable` | 标记/取消"待报销"（已发生报销的支出不可取消） |
| `POST/GET` | `/api/transactions/{id}/receipt` | 上传（multipart `file` 字段）或读取交易的小票/发票图片 |
| `POST` | `/api/reimbursements` | 报销支出（支持部分报销，生成关联收入流水；撤销支出会级联撤销报销收入） |
| `POST` | `/api/refunds` | 记录支出退款（指定到账账户，支持部分退款与跨币种结算；撤销原支出会级联撤销退款收入） |
| `GET` | `/api/tags` | 查询全部标签 |
| `GET/PUT/DELETE` | `/api/budgets` / `/api/budgets/{category_id}` | 查询/设置/清除某分类某月预算（`?year=&month=`） |
| `GET/POST` | `/api/recurring` | 查询或创建周期交易（每月/每周） |
| `POST` | `/api/recurring/run` | 手动触发到期周期交易生成（服务端也定时执行） |
| `PUT/DELETE` | `/api/recurring/{id}` | 编辑或删除周期交易 |
| `POST` | `/api/recurring/{id}/paused` | 暂停或恢复周期交易（`{"paused": true|false}`） |
| `GET` | `/api/recurring/{id}/preview` | 预览接下来三次发生日期 |
| `GET/POST` | `/api/holdings` | 查询股票持仓 |
| `GET` | `/api/holdings/quote?symbol=...` | 按证券代码查询参考价（Stooq → Yahoo Finance） |
| `POST` | `/api/holdings/refresh` | 刷新全部过期/缺失市价（Stooq → Yahoo Finance），返回逐标的明细 |
| `POST` | `/api/holdings/buy` / `/api/holdings/sell` | 买入/卖出股票（现金与持仓联动，支持 `fee` 手续费） |
| `PUT` | `/api/holdings/{id}/price` | 更新持仓市价 |
| `POST` | `/api/holdings/{id}/refresh` | 强制刷新单只持仓市价 |
| `POST` | `/api/deposits` | 储蓄转定期（利率 + 期限） |
| `POST` | `/api/deposits/{id}/settle` | 结清定期：按持有天数计息并把本息转回 |
| `GET/POST` | `/api/loans` | 查询或创建借出/借入 |
| `POST` | `/api/loans/{id}/repay` | 还款（任意账户进出，归零自动结清） |
| `GET/POST` | `/api/reconciliations` | 查询（`?account_id=`）或创建账户对账（对账单日期/目标余额/备注） |
| `POST` | `/api/reconciliations/{id}/complete` | 完成对账：差额非零时自动生成可审计的调整流水 |
| `POST` | `/api/reconciliations/{id}/cancel` | 取消对账（不产生调整） |
| `GET` | `/api/reminders` | 到期提醒：未来 `?days=`（默认 30）天内到期（含逾期）的定存、借款与信用卡账单 |
| `GET` | `/api/summary/monthly` | 按年月与币种查询收支；所有币种的流水统一按汇率折算到该币种 |
| `GET` | `/api/summary/cash-flow` | 查询收入来源、支出去向和结余现金流（多币种按汇率折算） |
| `GET` | `/api/summary/by-tag?tags=旅行,报销&year=&month=` | 标签汇总：同时带有全部指定标签（AND 语义）的收支合计与分类明细；缺省 year/month 统计全部历史 |
| `GET` | `/api/summary/trend` | 查询最近 `?months=`（默认 12，上限 120）个月的收支趋势，逐月返回收入/支出/结余 |
| `GET` | `/api/summary/yearly` | 年度汇总：`?year=`（缺省当前年）逐月收支 + 全年合计 + 收入/支出分类明细 |
| `GET` | `/api/summary/rolling` | 滚动平均：`?months=&window=`（默认 12/3）逐月给出 trailing window 的收入/支出/结余均值 |
| `GET` | `/api/summary/balance` | 按币种查询资产、负债与净值（所有币种账户与未结借款按汇率折算） |
| `GET` | `/api/rates?from=&to=` | 汇率提示：1 from = rate to（Frankfurter/ECB 参考中间价，本地缓存，源不可达时回退旧缓存） |
| `GET` | `/api/admin/backups` | （管理员）备份列表 |
| `POST` | `/api/admin/backup` | （管理员）立即创建备份（共享库 + 全部用户账本打包 zip） |
| `GET` | `/api/admin/backups/{id}/download` | （管理员）下载备份 zip |
| `POST` | `/api/admin/backups/{id}/restore` | （管理员）恢复备份（覆盖全部账本，所有会话失效） |
| `POST` | `/api/reminders/send` | 向当前用户邮箱手动发送其账本的到期提醒（需配置 SMTP） |
| `GET` | `/api/admin/r2/status` | （管理员）R2 状态：是否启用、桶/前缀、最近上传 |
| `POST` | `/api/admin/r2/upload/{id}` | （管理员）把某个本地备份补传到 R2 |
| `POST` | `/api/admin/r2/delete/{id}` | （管理员）删除 R2 上的备份对象（不影响本地） |
| `POST` | `/api/admin/r2/restore/{id}` | （管理员）从 R2 下载并恢复备份 |

`DELETE` 使用审计友好的软撤销语义，不物理删除交易记录。

创建收入/支出时通过 `currency` 指定原始交易币种。币种与账户结算币种不同时，同时提交 `settled_amount` 作为实际计入共享余额的金额，例如 `$10.00` 消费可按 `¥72.00` 入账。流水和月度收支按原始币种统计，账户余额始终使用账户结算币种。

## 进阶功能说明

### 交易导入（`/api/transactions/import`）

支持三种格式，`format` 缺省 `auto`（按文件扩展名/内容自动识别）：

- **CSV**：自动识别两类布局——Koku 自身导出的列（`kind/amount/currency/settled_amount/occurred_at/note/category`，支持导出→编辑→再导入的往返，非收支行自动跳过）与常见银行流水（日期/金额/备注列，支持 `date/日期/交易日期`、`amount/金额/发生额` 等中英文别名，可选 `类型/收支` 列显式指定方向）。
- **QIF**：`!Type:Bank/CCard/Cash` 段的 `D/T/P/M` 记录。
- **OFX**：`<STMTTRN>` 块（OFX 1.x SGML 与 2.x XML 均支持），取 `DTPOSTED/TRNAMT/NAME/MEMO/TRNTYPE`。

金额符号约定：负数 = 支出、正数 = 收入（QIF/OFX 的 `TRNTYPE` 借贷方向可覆盖符号）。导入按「账户 + 类型 + 时间 + 结算金额 + 备注」指纹去重；跨币种流水在行内未给结算金额时用本地缓存的汇率折算。单行失败不中断整批，错误明细逐行返回。导入文件与小票上传的上限均为 16 MiB。

### TOTP 二步验证

在「用户」页旁的侧边栏入口（钥匙图标旁的盾牌图标）可自助开启：输入当前密码 → 生成密钥（展示 Base32 密钥与 `otpauth://` URI，可用 Authenticator 扫码或粘贴）→ 输入一次 6 位动态码确认后启用。启用后登录分两步：先密码、后动态码；关闭同样需要当前动态码。开启后所有设备下次登录都必须通过二步验证。

### 备份/恢复

管理员在「系统」页可查看备份列表、立即备份、下载 zip、恢复。备份用 `VACUUM INTO` 在线一致性快照共享库与全部用户账本后打包；恢复会覆盖全部数据并使所有会话失效（恢复后需重新登录）。生产环境默认每天备份一次；可通过 `KOKU_BACKUP_INTERVAL_HOURS=0` 关闭，`KOKU_BACKUP_KEEP` 控制保留份数。配置 R2 后每次备份会自动上传异地副本。

### 持仓市价

「账户」页持仓区会显示市场、价格来源与价格日期；输入代码后买入窗可直接查询每股参考价，并可选择市场（默认自动识别）。代码可用裸代码或交易所后缀：`600519`/`600519.SS` 为沪市 A 股，`000001`/`000001.SZ` 为深市 A 股，`688981` 为科创板，`0700`/`0700.HK` 为港股，`AAPL`/`AAPL.US` 为美股。持仓唯一标识为“账户 + 市场 + 规范代码”：`NVDA` 与 `NVDA.US`、`700` 与 `0700.HK` 会自动合并；不同市场则始终独立。美股优先通过 Nasdaq 查询，其他市场优先 Stooq，失败再回退 Yahoo Finance；仍未覆盖的标的可手动填写市价，失败不会覆盖上一次有效价格。服务器会在 A 股/港股收盘后、以及美股收盘后按 `KOKU_JOBS_INTERVAL_MINUTES` 检查并刷新当天尚未更新的持仓，可用 `KOKU_QUOTE_AUTO_REFRESH=false` 关闭。买入和卖出窗均支持手续费：买入手续费计入持仓成本，卖出手续费从到账金额中扣除。

### R2 异地备份

配置 `KOKU_R2_ACCOUNT_ID/ACCESS_KEY_ID/SECRET_ACCESS_KEY/BUCKET/PREFIX` 后，每次备份（手动或定时）都会自动把 zip 上传到 Cloudflare R2（SigV4 签名，`PREFIX` 缺省 `koku`），并自动清理超出 `KOKU_BACKUP_KEEP` 的旧 R2 对象。「系统」页展示 R2 状态，支持补传、删除与**从 R2 恢复**（灾难恢复场景：服务器磁盘丢失后重建实例即可拉回备份）。

R2 API 令牌需在 Cloudflare 控制台创建并授予「对象读写」权限（建议只作用于备份桶）。境内服务器到 Cloudflare 实测约 67ms RTT、2.4MB/s 吞吐，个人账本的备份包（几 MB～几十 MB）上传无压力。

### 到期提醒

顶栏铃铛展示未来 30 天到期（含逾期）的定期存款、借款与未还信用卡账单。可选 SMTP 推送：配置 `KOKU_SMTP_HOST/PORT/TLS/USERNAME/PASSWORD/FROM` 后，服务端每 `KOKU_SMTP_INTERVAL_HOURS`（默认 24 小时）为每位启用用户读取其独立账本，并把到期摘要发至该用户的登录邮箱；用户也可在提醒面板手动发送自己的摘要。未配置 SMTP 时仅应用内提醒。
