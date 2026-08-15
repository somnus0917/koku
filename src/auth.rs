//! 登录配置与会话 Cookie/令牌工具。

use axum::http::{header, HeaderMap, HeaderValue};
use sha2::{Digest, Sha256};

use crate::config::{env_bool, required_env};
use crate::error::{KokuError, Result};

pub const SESSION_COOKIE_NAME: &str = "koku_session";

#[derive(Debug)]
pub struct AuthConfig {
    pub username: String,
    pub password_hash: String,
    pub session_ttl_seconds: i64,
    pub cookie_secure: bool,
}

impl AuthConfig {
    pub fn from_env() -> Result<Self> {
        let username = required_env("KOKU_AUTH_USERNAME")?;
        let password_hash = match std::env::var("KOKU_AUTH_PASSWORD_HASH") {
            Ok(value) if !value.trim().is_empty() => value.trim().to_owned(),
            Ok(_) | Err(std::env::VarError::NotPresent) => {
                let path = required_env("KOKU_AUTH_PASSWORD_HASH_FILE")?;
                std::fs::read_to_string(path)?.trim().to_owned()
            }
            Err(error) => {
                return Err(KokuError::AuthConfiguration(format!(
                    "could not read KOKU_AUTH_PASSWORD_HASH: {error}"
                )))
            }
        };
        bcrypt::verify("koku-password-hash-validation", &password_hash).map_err(|error| {
            KokuError::AuthConfiguration(format!("KOKU_AUTH_PASSWORD_HASH is invalid: {error}"))
        })?;
        let session_ttl_days = std::env::var("KOKU_SESSION_TTL_DAYS")
            .unwrap_or_else(|_| "30".to_owned())
            .parse::<i64>()
            .map_err(|error| {
                KokuError::AuthConfiguration(format!(
                    "KOKU_SESSION_TTL_DAYS must be an integer: {error}"
                ))
            })?;
        if !(1..=365).contains(&session_ttl_days) {
            return Err(KokuError::AuthConfiguration(
                "KOKU_SESSION_TTL_DAYS must be between 1 and 365".to_owned(),
            ));
        }
        Ok(Self {
            username,
            password_hash,
            session_ttl_seconds: session_ttl_days * 24 * 60 * 60,
            cookie_secure: env_bool("KOKU_COOKIE_SECURE", true)?,
        })
    }
}

/// 生成 256 位随机会话令牌（十六进制）。
pub fn generate_session_token() -> Result<String> {
    let mut random_bytes = [0_u8; 32];
    getrandom::fill(&mut random_bytes).map_err(|error| {
        KokuError::AuthConfiguration(format!("could not generate a session token: {error}"))
    })?;
    Ok(hex_encode(&random_bytes))
}

/// SQLite 只保存随机会话令牌的 SHA-256 摘要。
pub fn session_token_hash(token: &str) -> String {
    hex_encode(Sha256::digest(token.as_bytes()).as_ref())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

/// 从请求 Cookie 头解析会话令牌。
pub fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|cookie| cookie.trim().split_once('='))
        .find_map(|(name, value)| {
            (name == SESSION_COOKIE_NAME && !value.is_empty()).then(|| value.to_owned())
        })
}

/// 构造 HttpOnly/Secure/SameSite=Strict 会话 Cookie。
pub fn session_cookie(token: &str, max_age: i64, secure: bool) -> Result<HeaderValue> {
    let secure_attribute = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE_NAME}={token}; Path=/; HttpOnly{secure_attribute}; SameSite=Strict; Max-Age={max_age}"
    ))
    .map_err(|error| KokuError::AuthConfiguration(format!("invalid session cookie: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_cookie_is_http_only_and_parsed_by_name() -> Result<()> {
        let cookie = session_cookie("test-token", 3600, true)?;
        let cookie_text = cookie
            .to_str()
            .map_err(|error| KokuError::InvalidInput(error.to_string()))?;
        assert!(cookie_text.contains("HttpOnly"));
        assert!(cookie_text.contains("Secure"));
        assert!(cookie_text.contains("SameSite=Strict"));

        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("theme=dark; koku_session=test-token; locale=zh"),
        );
        assert_eq!(session_token(&headers).as_deref(), Some("test-token"));
        Ok(())
    }
}
