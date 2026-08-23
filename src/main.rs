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
mod r2;
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

use crate::api::{api_router, lock_auth, lock_ledger, AppState};
use crate::auth::AuthConfig;
use crate::config::{configured_origin, env_bool};
use crate::demo::seed_demo_data;
use crate::error::{KokuError, Result};
use crate::quotes::{should_refresh_after_close, QuoteClient};
use crate::r2::{R2Client, R2Config};
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
        maintenance: Arc::new(tokio::sync::RwLock::new(())),
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
        r2: R2Config::from_env()?.map(|config| Arc::new(R2Client::new(config))),
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

    // 周期交易和预算结转由服务端负责，不再依赖任一用户打开浏览器。默认每小时
    // 检查一次；每项业务自身幂等，因此重启或多次 tick 不会重复记账。
    let jobs_interval_minutes = std::env::var("KOKU_JOBS_INTERVAL_MINUTES")
        .unwrap_or_else(|_| "60".to_owned())
        .parse::<u64>()
        .map_err(|error| {
            KokuError::AuthConfiguration(format!(
                "KOKU_JOBS_INTERVAL_MINUTES must be an integer: {error}"
            ))
        })?;
    if !(1..=24 * 60).contains(&jobs_interval_minutes) {
        return Err(KokuError::AuthConfiguration(
            "KOKU_JOBS_INTERVAL_MINUTES must be between 1 and 1440".to_owned(),
        ));
    }
    let quote_auto_refresh = env_bool("KOKU_QUOTE_AUTO_REFRESH", true)?;
    {
        let jobs_state = state.clone();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_secs(jobs_interval_minutes * 60));
            loop {
                interval.tick().await;
                let _maintenance = jobs_state.maintenance.read().await;
                let users = match lock_auth(&jobs_state).and_then(|service| service.users()) {
                    Ok(users) => users,
                    Err(error) => {
                        tracing::error!(target: "jobs", error = %error, "could not list users for scheduled jobs");
                        continue;
                    }
                };
                for user in users.into_iter().filter(|user| user.enabled) {
                    match lock_ledger(&jobs_state, user.id).await {
                        Ok(mut ledger) => {
                            if let Err(error) = ledger.run_recurring() {
                                tracing::error!(target: "jobs", user_id = user.id, error = %error, "recurring job failed");
                            }
                            if let Err(error) = ledger.rollover_budgets_once(chrono::Utc::now()) {
                                tracing::error!(target: "jobs", user_id = user.id, error = %error, "budget rollover failed");
                            }
                            if quote_auto_refresh {
                                let now = chrono::Utc::now();
                                let due = match ledger.holdings() {
                                    Ok(holdings) => holdings
                                        .into_iter()
                                        .filter(|holding| {
                                            should_refresh_after_close(
                                                &holding.market,
                                                holding.updated_at,
                                                now,
                                            )
                                        })
                                        .map(|holding| (holding.id, holding.symbol))
                                        .collect::<Vec<_>>(),
                                    Err(error) => {
                                        tracing::error!(target: "jobs", user_id = user.id, error = %error, "could not list holdings for quote refresh");
                                        Vec::new()
                                    }
                                };
                                drop(ledger);
                                for (holding_id, symbol) in due {
                                    match jobs_state.quotes.fetch(&symbol).await {
                                        Ok(quote) => {
                                            match lock_ledger(&jobs_state, user.id).await {
                                                Ok(mut ledger) => {
                                                    if let Err(error) =
                                                        ledger.set_holding_quote(holding_id, &quote)
                                                    {
                                                        tracing::warn!(target: "jobs", user_id = user.id, holding_id, error = %error, "could not save scheduled quote")
                                                    }
                                                }
                                                Err(error) => {
                                                    tracing::warn!(target: "jobs", user_id = user.id, holding_id, error = %error, "could not open ledger to save scheduled quote")
                                                }
                                            }
                                        }
                                        Err(error) => {
                                            tracing::warn!(target: "jobs", user_id = user.id, symbol = %symbol, error = %error, "scheduled quote refresh failed; retaining last valid price")
                                        }
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            tracing::error!(target: "jobs", user_id = user.id, error = %error, "could not open ledger for scheduled jobs")
                        }
                    }
                }
            }
        });
    }

    // 定时备份：默认每天运行；显式设为 0 可关闭（仅手动触发）。
    let backup_interval_hours = std::env::var("KOKU_BACKUP_INTERVAL_HOURS")
        .unwrap_or_else(|_| "24".to_owned())
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
        let backup_r2 = state.r2.clone();
        let backup_maintenance = state.maintenance.clone();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_secs(backup_interval_hours * 3600));
            // 首个 tick 立即触发，这里主动跳过，让部署脚本/手动触发负责启动时机。
            interval.tick().await;
            loop {
                interval.tick().await;
                let created = {
                    let _maintenance = backup_maintenance.write().await;
                    backup::create_backup(&backup_db_path, &backup_ledger_dir, backup_keep)
                };
                match created {
                    Ok(meta) => {
                        // 配置了 R2 时上传本次备份并清理超出保留份数的旧对象。
                        if let Some(r2) = &backup_r2 {
                            let dir = backup::backup_dir(&backup_db_path);
                            let key = r2.object_key(&meta.filename);
                            let upload = async {
                                let bytes = std::fs::read(dir.join(&meta.filename))
                                    .map_err(KokuError::from)?;
                                r2.put_object(&key, &bytes, "application/zip").await
                            };
                            match upload.await {
                                Ok(()) => {
                                    tracing::info!(target: "koku", backup = %meta.id, "scheduled backup uploaded to R2");
                                    r2::prune_old_objects(r2, &backup_db_path, backup_keep).await;
                                }
                                Err(error) => {
                                    tracing::warn!(target: "koku", error = %error, "scheduled backup R2 upload failed")
                                }
                            }
                        }
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
    // 为每个启用用户读取其独立账本，并发送到其登录邮箱。
    if let Some(mailer_config) = mailer::MailerConfig::from_env()? {
        let smtp_interval_hours = std::env::var("KOKU_SMTP_INTERVAL_HOURS")
            .unwrap_or_else(|_| "24".to_owned())
            .parse::<u64>()
            .map_err(|error| {
                KokuError::InvalidInput(format!(
                    "KOKU_SMTP_INTERVAL_HOURS must be an integer: {error}"
                ))
            })?;
        let smtp_state = state.clone();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_secs(smtp_interval_hours * 3600));
            interval.tick().await;
            loop {
                interval.tick().await;
                let users = match lock_auth(&smtp_state).and_then(|service| service.users()) {
                    Ok(users) => users,
                    Err(error) => {
                        tracing::error!(target: "koku", error = %error, "could not list users for reminder digest");
                        continue;
                    }
                };
                for user in users.into_iter().filter(|user| user.enabled) {
                    let items = match lock_ledger(&smtp_state, user.id).await {
                        Ok(mut ledger) => ledger.due_reminders(30),
                        Err(error) => {
                            tracing::error!(target: "koku", user_id = user.id, error = %error, "could not open ledger for reminder digest");
                            continue;
                        }
                    };
                    let items = match items {
                        Ok(items) if !items.is_empty() => items,
                        Ok(_) => continue,
                        Err(error) => {
                            tracing::error!(target: "koku", user_id = user.id, error = %error, "could not load reminder digest");
                            continue;
                        }
                    };
                    let subject = format!("Koku 到期提醒（{} 项）", items.len());
                    let body = service::reminder_digest_text(&items);
                    let config = mailer_config.clone();
                    let recipient = user.username.clone();
                    match tokio::task::spawn_blocking(move || {
                        mailer::send_mail(&config, &recipient, &subject, &body)
                    })
                    .await
                    .map_err(|error| {
                        KokuError::AuthConfiguration(format!("smtp task failed: {error}"))
                    })
                    .and_then(|result| result)
                    {
                        Ok(()) => {
                            tracing::info!(target: "koku", user_id = user.id, "scheduled reminder digest sent")
                        }
                        Err(error) => {
                            tracing::error!(target: "koku", user_id = user.id, error = %error, "scheduled reminder digest failed")
                        }
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
