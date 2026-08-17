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
mod backup;
mod config;
mod demo;
mod domain;
mod error;
mod importer;
mod mailer;
mod quotes;
mod ratelimit;
mod rates;
mod service;
mod throttle;
mod totp;

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::api::{api_router, AppState};
use crate::auth::AuthConfig;
use crate::config::{configured_origin, env_bool};
use crate::demo::seed_demo_data;
use crate::error::{KokuError, Result};
use crate::quotes::QuoteClient;
use crate::ratelimit::ApiRateLimiter;
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
        ledgers: Arc::new(Mutex::new(HashMap::new())),
        ledger_dir: ledger_dir.clone(),
        db_path: Path::new(&database_path).to_path_buf(),
        auth_config: Arc::new(auth_config),
        login_throttle: Arc::new(Mutex::new(LoginThrottle::default())),
        rate_limiter: Arc::new(Mutex::new(ApiRateLimiter::from_env()?)),
        pending_totp: Arc::new(Mutex::new(HashMap::new())),
        rates: Arc::new(RateClient::new()),
        quotes: Arc::new(QuoteClient::new()),
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

    // 定时备份：KOKU_BACKUP_INTERVAL_HOURS > 0 时启用（0 表示关闭，仅手动触发）。
    let backup_interval_hours = std::env::var("KOKU_BACKUP_INTERVAL_HOURS")
        .unwrap_or_else(|_| "0".to_owned())
        .parse::<u64>()
        .map_err(|error| {
            KokuError::InvalidInput(format!(
                "KOKU_BACKUP_INTERVAL_HOURS must be an integer: {error}"
            ))
        })?;
    let backup_keep = std::env::var("KOKU_BACKUP_KEEP")
        .unwrap_or_else(|_| "14".to_owned())
        .parse::<usize>()
        .map_err(|error| {
            KokuError::InvalidInput(format!("KOKU_BACKUP_KEEP must be an integer: {error}"))
        })?;
    if backup_interval_hours > 0 {
        let backup_db_path = state.db_path.clone();
        let backup_ledger_dir = state.ledger_dir.clone();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_secs(backup_interval_hours * 3600));
            // 首个 tick 立即触发，这里主动跳过，让部署脚本/手动触发负责启动时机。
            interval.tick().await;
            loop {
                interval.tick().await;
                match backup::create_backup(&backup_db_path, &backup_ledger_dir, backup_keep) {
                    Ok(meta) => {
                        tracing::info!(target: "koku", backup = %meta.id, "scheduled backup completed")
                    }
                    Err(error) => {
                        tracing::error!(target: "koku", error = %error, "scheduled backup failed")
                    }
                }
            }
        });
    }

    // 到期提醒邮件：配置了 SMTP 时按 KOKU_SMTP_INTERVAL_HOURS（默认 24 小时）
    // 把管理员账本中 30 天内的到期提醒发到 KOKU_SMTP_TO。
    if let Some(mailer_config) = mailer::MailerConfig::from_env()? {
        let smtp_interval_hours = std::env::var("KOKU_SMTP_INTERVAL_HOURS")
            .unwrap_or_else(|_| "24".to_owned())
            .parse::<u64>()
            .map_err(|error| {
                KokuError::InvalidInput(format!(
                    "KOKU_SMTP_INTERVAL_HOURS must be an integer: {error}"
                ))
            })?;
        let smtp_admin_id = admin_id;
        let smtp_ledger_dir = ledger_dir.clone();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_secs(smtp_interval_hours * 3600));
            interval.tick().await;
            loop {
                interval.tick().await;
                let result: Result<()> = async {
                    let ledger = BookkeepingService::open(
                        smtp_ledger_dir.join(format!("ledger-{smtp_admin_id}.db")),
                    )?;
                    let items = ledger.due_reminders(30)?;
                    if items.is_empty() {
                        return Ok(());
                    }
                    let subject = format!("Koku 到期提醒（{} 项）", items.len());
                    let body = service::reminder_digest_text(&items);
                    let config = mailer_config.clone();
                    tokio::task::spawn_blocking(move || {
                        mailer::send_mail(&config, &subject, &body)
                    })
                    .await
                    .map_err(|error| {
                        KokuError::AuthConfiguration(format!("smtp task failed: {error}"))
                    })??;
                    Ok(())
                }
                .await;
                match result {
                    Ok(()) => tracing::info!(target: "koku", "scheduled reminder digest sent"),
                    Err(error) => {
                        tracing::error!(target: "koku", error = %error, "scheduled reminder digest failed")
                    }
                }
            }
        });
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
