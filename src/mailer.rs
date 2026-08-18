//! 可选 SMTP 邮件提醒：配置 `KOKU_SMTP_*` 环境变量后，
//! 定时/手动向指定收件人发送到期提醒摘要。
//!
//! 完全可选：未配置 `KOKU_SMTP_HOST` 时整个模块不生效（应用内提醒仍可用）。

use lettre::message::{Mailbox, Message};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{SmtpTransport, Transport};

use crate::error::{KokuError, Result};

/// SMTP 发送配置（从环境变量解析）。
#[derive(Debug, Clone)]
pub struct MailerConfig {
    pub host: String,
    pub port: u16,
    /// "starttls"（587 默认）| "implicit"（465 默认）| "none"
    pub tls: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from: String,
    pub to: String,
}

impl MailerConfig {
    /// 解析环境变量；未设置 `KOKU_SMTP_HOST` 时返回 `Ok(None)`（不启用）。
    /// 设置了 host 但缺少 from/to 时报错（配置不完整）。
    pub fn from_env() -> Result<Option<Self>> {
        let host = match std::env::var("KOKU_SMTP_HOST") {
            Ok(value) if !value.trim().is_empty() => value.trim().to_owned(),
            Ok(_) | Err(std::env::VarError::NotPresent) => return Ok(None),
            Err(error) => {
                return Err(KokuError::AuthConfiguration(format!(
                    "could not read KOKU_SMTP_HOST: {error}"
                )))
            }
        };
        let required = |name: &str| -> Result<String> {
            std::env::var(name)
                .map(|value| value.trim().to_owned())
                .map_err(|_| {
                    KokuError::AuthConfiguration(format!(
                        "{name} is required when KOKU_SMTP_HOST is set"
                    ))
                })
        };
        let port = std::env::var("KOKU_SMTP_PORT")
            .unwrap_or_else(|_| "587".to_owned())
            .parse::<u16>()
            .map_err(|error| {
                KokuError::AuthConfiguration(format!("KOKU_SMTP_PORT must be a port: {error}"))
            })?;
        let tls = std::env::var("KOKU_SMTP_TLS")
            .unwrap_or_else(|_| "starttls".to_owned())
            .to_ascii_lowercase();
        if !matches!(tls.as_str(), "starttls" | "implicit" | "none") {
            return Err(KokuError::AuthConfiguration(
                "KOKU_SMTP_TLS must be starttls, implicit, or none".to_owned(),
            ));
        }
        Ok(Some(Self {
            host,
            port,
            tls,
            username: std::env::var("KOKU_SMTP_USERNAME").ok(),
            password: std::env::var("KOKU_SMTP_PASSWORD").ok(),
            from: required("KOKU_SMTP_FROM")?,
            to: required("KOKU_SMTP_TO")?,
        }))
    }
}

/// 发送一封纯文本邮件；失败返回带上下文的错误（不 panic）。
pub fn send_mail(config: &MailerConfig, subject: &str, body: &str) -> Result<()> {
    let from: Mailbox = config
        .from
        .parse()
        .map_err(|error| KokuError::InvalidInput(format!("invalid KOKU_SMTP_FROM: {error}")))?;
    let to: Mailbox = config
        .to
        .parse()
        .map_err(|error| KokuError::InvalidInput(format!("invalid KOKU_SMTP_TO: {error}")))?;
    let message = Message::builder()
        .from(from)
        .to(to)
        .subject(subject.to_owned())
        .body(body.to_owned())
        .map_err(|error| KokuError::InvalidInput(format!("invalid email message: {error}")))?;

    let mailer = build_transport(config)?;
    mailer
        .send(&message)
        .map_err(|error| KokuError::InvalidInput(format!("smtp send failed: {error}")))?;
    Ok(())
}

fn build_transport(config: &MailerConfig) -> Result<SmtpTransport> {
    let builder = match config.tls.as_str() {
        "implicit" => SmtpTransport::relay(&config.host)
            .map_err(|error| KokuError::AuthConfiguration(format!("smtp relay failed: {error}")))?,
        "none" => SmtpTransport::builder_dangerous(&config.host),
        _ => SmtpTransport::starttls_relay(&config.host).map_err(|error| {
            KokuError::AuthConfiguration(format!("smtp starttls relay failed: {error}"))
        })?,
    }
    .port(config.port);
    let builder = match (&config.username, &config.password) {
        (Some(username), Some(password)) => {
            builder.credentials(Credentials::new(username.clone(), password.clone()))
        }
        _ => builder,
    };
    Ok(builder.build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 多个测试读写同一组环境变量，必须串行执行避免互相干扰。
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn mailer_config_is_absent_without_host() -> Result<()> {
        let _guard = ENV_LOCK.lock().unwrap();
        // 测试环境变量可控：确保 KOKU_SMTP_HOST 未设置。
        std::env::remove_var("KOKU_SMTP_HOST");
        assert!(MailerConfig::from_env()?.is_none());
        Ok(())
    }

    #[test]
    fn mailer_config_requires_from_and_to_when_host_is_set() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("KOKU_SMTP_HOST", "smtp.example.com");
        std::env::remove_var("KOKU_SMTP_FROM");
        std::env::remove_var("KOKU_SMTP_TO");
        assert!(MailerConfig::from_env().is_err());
        std::env::set_var("KOKU_SMTP_FROM", "a@example.com");
        std::env::set_var("KOKU_SMTP_TO", "b@example.com");
        assert!(MailerConfig::from_env().is_ok());
        std::env::remove_var("KOKU_SMTP_HOST");
    }

    #[test]
    fn rejects_unknown_tls_modes() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("KOKU_SMTP_HOST", "smtp.example.com");
        std::env::set_var("KOKU_SMTP_FROM", "a@example.com");
        std::env::set_var("KOKU_SMTP_TO", "b@example.com");
        std::env::set_var("KOKU_SMTP_TLS", "sneaky");
        assert!(MailerConfig::from_env().is_err());
        std::env::set_var("KOKU_SMTP_TLS", "starttls");
        assert!(MailerConfig::from_env().is_ok());
        std::env::remove_var("KOKU_SMTP_HOST");
        std::env::remove_var("KOKU_SMTP_TLS");
    }
}
