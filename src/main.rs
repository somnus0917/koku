//! Koku API 进程入口与服务器启动。
//!
//! 业务逻辑按职责拆分到以下模块：
//! - `domain`  领域类型（账户/分类/交易枚举与 DTO）
//! - `service` SQLite 持久化、记账业务、软撤销与迁移
//! - `api`     REST 处理器、鉴权中间件与路由
//! - `auth`    登录配置与会话 Cookie/令牌工具
//! - `config`  环境变量解析
//! - `throttle` 登录失败限流
//! - `demo`    控制台演示与演示账本种子
//! - `error`   统一错误类型与 HTTP 映射

mod api;
mod auth;
mod config;
mod demo;
mod domain;
mod error;
mod rates;
mod service;
mod throttle;

use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use crate::api::{api_router, AppState};
use crate::auth::AuthConfig;
use crate::config::{configured_origin, env_bool};
use crate::demo::seed_demo_data;
use crate::error::{KokuError, Result};
use crate::rates::RateClient;
use crate::service::{ensure_multi_user, BookkeepingService};
use crate::throttle::LoginThrottle;

async fn run_server() -> Result<()> {
    let database_path = std::env::var("KOKU_DB_PATH").unwrap_or_else(|_| "data/koku.db".to_owned());
    if let Some(parent) = Path::new(&database_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 每个用户一个独立账本文件，放在共享库同目录的 ledgers/ 下。
    let ledger_dir = Path::new(&database_path)
        .parent()
        .unwrap_or_else(|| Path::new("data"))
        .join("ledgers");
    std::fs::create_dir_all(&ledger_dir)?;

    let auth_config = AuthConfig::from_env()?;
    let mut auth_service = BookkeepingService::open(&database_path)?;
    // 多用户迁移：引导 admin（现有单用户凭据）、回填会话、搬迁旧账本数据。
    let admin_id = ensure_multi_user(
        &mut auth_service,
        Path::new(&database_path),
        &ledger_dir,
        &auth_config.username,
        &auth_config.password_hash,
    )?;

    let state = AppState {
        auth: Arc::new(Mutex::new(auth_service)),
        ledger_dir: ledger_dir.clone(),
        auth_config: Arc::new(auth_config),
        login_throttle: Arc::new(Mutex::new(LoginThrottle::default())),
        rates: Arc::new(RateClient::new()),
    };

    // 演示账本：仅在全新安装（admin 账本为空）时填充。
    if env_bool("KOKU_SEED_DEMO", true)? {
        let admin_ledger_path = ledger_dir.join(format!("ledger-{admin_id}.db"));
        let mut admin_ledger = BookkeepingService::open(&admin_ledger_path)?;
        admin_ledger.ensure_default_categories()?;
        if admin_ledger.is_empty()? {
            seed_demo_data(&mut admin_ledger)?;
        }
    }

    let host = std::env::var("KOKU_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let host = IpAddr::from_str(&host)
        .map_err(|error| KokuError::InvalidInput(format!("invalid KOKU_HOST: {error}")))?;
    let port = std::env::var("KOKU_PORT")
        .unwrap_or_else(|_| "8080".to_owned())
        .parse::<u16>()
        .map_err(|error| KokuError::InvalidInput(format!("invalid KOKU_PORT: {error}")))?;
    let address = SocketAddr::new(host, port);
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Koku API is listening on http://{address}");

    axum::serve(
        listener,
        api_router(state, configured_origin()?).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            eprintln!("failed to listen for shutdown signal: {error}");
        }
    })
    .await?;
    Ok(())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "auth=info,koku=info,tower_http=info".into()),
        )
        .init();
    let result = if std::env::args().any(|argument| argument == "--demo") {
        demo::run_demo()
    } else {
        run_server().await
    };
    if let Err(error) = result {
        eprintln!("Koku failed: {error}");
        std::process::exit(1);
    }
}
