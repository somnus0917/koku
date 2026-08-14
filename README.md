# Koku

Koku 是一个本地优先、前后端分离的个人记账 MVP。设计借鉴了 [Sure](https://github.com/we-promise/sure) 克制、清晰的财务工作台体验，但采用独立的 Koku 视觉语言与 Rust 实现。

## 功能

- 资产与负债账户，多币种独立统计
- 收入、支出、同币种与跨币种转账
- SQLite 原子余额更新与账单撤销
- `rust_decimal` 精确货币计算，API 金额统一序列化为字符串
- 月度收支、净结余、分类占比与净资产统计
- 28 个开箱即用的收入/支出分类，并支持自定义补充
- 桌面 Sankey 与手机纵向流量卡片两套现金流视图
- 响应式桌面/移动界面、底部快捷导航、浅色/深色主题
- 本地 SQLite 持久化，首次启动自动生成演示账本

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

保留原始控制台演示入口：

```bash
cargo run -- --demo
```

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
