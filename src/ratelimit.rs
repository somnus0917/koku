//! 通用 API 限流：固定窗口按客户端计数，防止登录后的业务接口被高频刷取
//! （如 Cookie 泄露后批量读取账本、拖库式导出）。
//!
//! 与 [`crate::throttle::LoginThrottle`] 共用同一套客户端标识解析
//! （`X-Forwarded-For` 首段，须由边缘代理重置为真实客户端 IP），
//! 实现上也保持一致的自包含风格：不引入 `tower-governor` 等重量级依赖，
//! 保持 Cargo 依赖树最小（本项目定位为最小化、私有化的自托管应用）。
//!
//! 窗口为固定 60 秒，默认每客户端每分钟 300 次（`KOKU_RATE_LIMIT_PER_MINUTE`，
//! 设 0 可完全关闭）。健康检查接口不参与限流。

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::header;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::api::AppState;
use crate::error::KokuError;
use crate::throttle::LoginThrottle;

/// 默认限流：每客户端每分钟 300 次请求。
const DEFAULT_LIMIT_PER_MINUTE: u32 = 300;
const WINDOW: Duration = Duration::from_secs(60);
/// 防内存无限增长：超过该条目数时顺手清理过期键。
const MAX_ENTRIES: usize = 2048;

pub struct ApiRateLimiter {
    requests: HashMap<String, (Instant, u32)>,
    limit_per_minute: u32,
}

impl Default for ApiRateLimiter {
    fn default() -> Self {
        Self {
            requests: HashMap::new(),
            limit_per_minute: DEFAULT_LIMIT_PER_MINUTE,
        }
    }
}

impl ApiRateLimiter {
    /// 从环境变量构建；`KOKU_RATE_LIMIT_PER_MINUTE=0` 表示关闭限流。
    pub fn from_env() -> crate::error::Result<Self> {
        let limit = match std::env::var("KOKU_RATE_LIMIT_PER_MINUTE") {
            Ok(value) => value.parse::<u32>().map_err(|error| {
                KokuError::InvalidInput(format!(
                    "KOKU_RATE_LIMIT_PER_MINUTE must be an integer: {error}"
                ))
            })?,
            Err(std::env::VarError::NotPresent) => DEFAULT_LIMIT_PER_MINUTE,
            Err(error) => {
                return Err(KokuError::InvalidInput(format!(
                    "could not read KOKU_RATE_LIMIT_PER_MINUTE: {error}"
                )))
            }
        };
        Ok(Self {
            requests: HashMap::new(),
            limit_per_minute: limit,
        })
    }

    /// 记账一次请求；超过窗口上限返回 `false`（应返回 429）。
    pub fn record(&mut self, key: &str) -> bool {
        if self.limit_per_minute == 0 {
            return true;
        }
        let now = Instant::now();
        if self.requests.len() >= MAX_ENTRIES {
            self.requests
                .retain(|_, (started_at, _)| now.duration_since(*started_at) < WINDOW);
        }
        match self.requests.get_mut(key) {
            Some((started_at, count)) => {
                if now.duration_since(*started_at) >= WINDOW {
                    *started_at = now;
                    *count = 1;
                    true
                } else if *count >= self.limit_per_minute {
                    false
                } else {
                    *count += 1;
                    true
                }
            }
            None => {
                self.requests.insert(key.to_owned(), (now, 1));
                true
            }
        }
    }

    /// 剩余可用配额（测试用）。
    #[cfg(test)]
    pub fn remaining(&self, key: &str) -> u32 {
        if self.limit_per_minute == 0 {
            return u32::MAX;
        }
        match self.requests.get(key) {
            Some((started_at, count)) => {
                if started_at.elapsed() >= WINDOW {
                    self.limit_per_minute
                } else {
                    self.limit_per_minute.saturating_sub(*count)
                }
            }
            None => self.limit_per_minute,
        }
    }
}

/// 请求级限流中间件：挂在整棵路由上，除健康检查外按客户端键计数。
/// 超限返回 429 + `Retry-After`（窗口剩余秒数）。
pub async fn rate_limit(
    State(state): State<AppState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    if request.uri().path() == "/api/health" {
        return next.run(request).await;
    }
    let key = LoginThrottle::client_key(request.headers(), Some(remote.ip()));
    let allowed = state
        .rate_limiter
        .lock()
        .map(|mut limiter| limiter.record(&key))
        .unwrap_or(true);
    if !allowed {
        tracing::warn!(target: "koku", "rate limited client {key}");
        let mut response = KokuError::RateLimited.into_response();
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
        return response;
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_limit_and_rolls_window() {
        let mut limiter = ApiRateLimiter {
            requests: HashMap::new(),
            limit_per_minute: 3,
        };
        assert!(limiter.record("client-a"));
        assert!(limiter.record("client-a"));
        assert!(limiter.record("client-a"));
        // 第 4 次超限
        assert!(!limiter.record("client-a"));
        // 其他客户端不受影响
        assert!(limiter.record("client-b"));
        // 窗口滚动后恢复（直接把起始时间拨回窗口外）
        if let Some((started_at, _)) = limiter.requests.get_mut("client-a") {
            *started_at = Instant::now() - WINDOW - Duration::from_secs(1);
        }
        assert!(limiter.record("client-a"));
    }

    #[test]
    fn zero_limit_disables_throttling() {
        let mut limiter = ApiRateLimiter {
            requests: HashMap::new(),
            limit_per_minute: 0,
        };
        for _ in 0..10_000 {
            assert!(limiter.record("client-a"));
        }
    }

    #[test]
    fn remaining_reflects_quota() {
        let mut limiter = ApiRateLimiter {
            requests: HashMap::new(),
            limit_per_minute: 10,
        };
        assert_eq!(limiter.remaining("client-a"), 10);
        limiter.record("client-a");
        limiter.record("client-a");
        assert_eq!(limiter.remaining("client-a"), 8);
    }
}
