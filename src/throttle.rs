//! 登录失败限流：固定窗口计数，防止对公网单账户系统的暴力破解。
//!
//! 生产环境位于 edge-caddy 之后，`RemoteAddr` 恒为反向代理地址，因此用
//! `X-Forwarded-For` 首段作为客户端标识。**前提契约**：边缘代理（Caddy）
//! 必须在离客户端最近的一跳用 `header_up X-Forwarded-For
//! {http.request.remote.host}` 把该头重置为真实客户端 IP（见
//! `deploy/Caddyfile.example`）。Caddy/nginx 默认都是追加而非覆盖，若未重置，
//! 攻击者自带的伪造首段会直接成为限流键，每次换值即可绕过限流。
//! 取不到该头时回退到对端地址（单用户场景下等价于全局限流）。

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use axum::http::HeaderMap;

/// 窗口长度：5 分钟内同一来源最多失败 5 次。
const WINDOW: Duration = Duration::from_secs(300);
const MAX_FAILURES: u32 = 5;
/// 防内存无限增长：超过该条目数时顺手清理过期键。
const MAX_ENTRIES: usize = 1024;

#[derive(Default)]
pub struct LoginThrottle {
    failures: HashMap<String, (Instant, u32)>,
}

impl LoginThrottle {
    /// 解析限流键：取 `X-Forwarded-For` 首段（须由边缘代理重置为真实客户端 IP），
    /// 其次对端 IP。
    pub fn client_key(headers: &HeaderMap, remote: Option<IpAddr>) -> String {
        headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .or_else(|| remote.map(|ip| ip.to_string()))
            .unwrap_or_else(|| "unknown".to_owned())
    }

    /// 登录尝试记账。`success=true` 清除该来源的失败计数；
    /// `success=false` 累加失败次数，达到上限后返回 `Err(())`（应返回 429）。
    pub fn record(&mut self, key: &str, success: bool) -> std::result::Result<(), ()> {
        let now = Instant::now();
        if self.failures.len() >= MAX_ENTRIES {
            self.failures
                .retain(|_, (started_at, _)| now.duration_since(*started_at) < WINDOW);
        }
        match self.failures.get_mut(key) {
            Some((started_at, count)) => {
                if now.duration_since(*started_at) >= WINDOW {
                    *started_at = now;
                    *count = 0;
                }
                if success {
                    self.failures.remove(key);
                    Ok(())
                } else if *count >= MAX_FAILURES {
                    Err(())
                } else {
                    *count += 1;
                    Ok(())
                }
            }
            None => {
                if success {
                    Ok(())
                } else {
                    self.failures.insert(key.to_owned(), (now, 1));
                    Ok(())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> String {
        LoginThrottle::client_key(&HeaderMap::new(), Some("203.0.113.9".parse().unwrap()))
    }

    #[test]
    fn blocks_after_max_failures_and_resets_after_success() {
        let mut throttle = LoginThrottle::default();
        let key = key();
        for _ in 0..MAX_FAILURES {
            assert!(throttle.record(&key, false).is_ok());
        }
        // 第 MAX_FAILURES+1 次起被锁
        assert!(throttle.record(&key, false).is_err());
        assert!(throttle.record(&key, false).is_err());
        // 成功登录解除锁定
        assert!(throttle.record(&key, true).is_ok());
        assert!(throttle.record(&key, false).is_ok());
    }

    #[test]
    fn reads_first_x_forwarded_for_entry() {
        // 边缘 Caddy 已把头重置为真实客户端 IP，nginx 再追加一跳是安全的：
        // 限流键取首段（真实客户端 IP），而不是被追加的代理内网地址。
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "198.51.100.7, 10.0.0.2".parse().unwrap());
        assert_eq!(
            LoginThrottle::client_key(&headers, Some("10.0.0.2".parse().unwrap())),
            "198.51.100.7"
        );
    }
}
