//! 环境变量解析与运行配置。

use axum::http::HeaderValue;

use crate::error::{KokuError, Result};

pub fn parse_env_bool(name: &str, value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(KokuError::InvalidInput(format!(
            "{name} must be one of true, false, 1, 0, yes, no, on, or off"
        ))),
    }
}

pub fn env_bool(name: &str, default: bool) -> Result<bool> {
    match std::env::var(name) {
        Ok(value) => parse_env_bool(name, &value),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(KokuError::InvalidInput(format!(
            "could not read {name}: {error}"
        ))),
    }
}

pub fn required_env(name: &str) -> Result<String> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(value.trim().to_owned()),
        Ok(_) | Err(std::env::VarError::NotPresent) => Err(KokuError::AuthConfiguration(format!(
            "{name} is required and cannot be empty"
        ))),
        Err(error) => Err(KokuError::AuthConfiguration(format!(
            "could not read {name}: {error}"
        ))),
    }
}

/// 可选 CORS 来源；同域生产部署无需开启 CORS。
pub fn configured_origin() -> Result<Option<HeaderValue>> {
    match std::env::var("KOKU_ALLOWED_ORIGIN") {
        Ok(value) if !value.trim().is_empty() => {
            value.parse::<HeaderValue>().map(Some).map_err(|error| {
                KokuError::InvalidInput(format!("invalid KOKU_ALLOWED_ORIGIN: {error}"))
            })
        }
        Ok(_) | Err(std::env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(KokuError::InvalidInput(format!(
            "could not read KOKU_ALLOWED_ORIGIN: {error}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_boolean_flags_are_strict_and_case_insensitive() -> Result<()> {
        assert!(parse_env_bool("FLAG", "TRUE")?);
        assert!(parse_env_bool("FLAG", "yes")?);
        assert!(!parse_env_bool("FLAG", "0")?);
        assert!(!parse_env_bool("FLAG", "Off")?);
        assert!(matches!(
            parse_env_bool("FLAG", "sometimes"),
            Err(KokuError::InvalidInput(_))
        ));
        Ok(())
    }
}
