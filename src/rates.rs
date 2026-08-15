//! 汇率提示客户端：多数据源拉取并解析，主源 Frankfurter（ECB 参考汇率），
//! 备用源 fawazahmed0/currency-api（GitHub 开源托管），提升网络可用性。
//!
//! 两个源都免费、无需 key、每日更新且支持人民币。
//! 本模块只负责「拉取 + 解析」；SQLite 缓存读写见 `service::rates`。

use std::collections::HashMap;
use std::time::Duration;

use chrono::NaiveDate;
use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use serde::Deserialize;

use crate::domain::RateQuote;
use crate::error::{KokuError, Result};

/// 共享的汇率 HTTP 客户端（内部复用连接）。
#[derive(Debug, Clone)]
pub struct RateClient {
    client: reqwest::Client,
    frankfurter_base: String,
    currency_api_base: String,
}

impl Default for RateClient {
    fn default() -> Self {
        Self::new()
    }
}

impl RateClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(10))
                .user_agent("koku/0.1 (personal ledger)")
                .build()
                .expect("failed to build exchange rate http client"),
            frankfurter_base: "https://api.frankfurter.app".to_owned(),
            currency_api_base:
                "https://cdn.jsdelivr.net/npm/@fawazahmed0/currency-api@latest/v1/currencies"
                    .to_owned(),
        }
    }

    /// 拉取最新参考汇率：1 from = rate to。主源失败时自动切换到备用源。
    pub async fn fetch(&self, from: &str, to: &str) -> Result<RateQuote> {
        match self.fetch_frankfurter(from, to).await {
            Ok(quote) => Ok(quote),
            Err(first) => match self.fetch_currency_api(from, to).await {
                Ok(quote) => Ok(quote),
                Err(second) => Err(KokuError::RateSource(format!(
                    "all exchange rate sources failed: {first}; {second}"
                ))),
            },
        }
    }

    async fn fetch_frankfurter(&self, from: &str, to: &str) -> Result<RateQuote> {
        let url = format!("{}/latest?from={from}&to={to}", self.frankfurter_base);
        let response = self.client.get(&url).send().await.map_err(|error| {
            KokuError::RateSource(format!("frankfurter request failed: {error}"))
        })?;
        if !response.status().is_success() {
            return Err(KokuError::RateSource(format!(
                "frankfurter returned HTTP {}",
                response.status()
            )));
        }
        let payload: FrankfurterResponse = response.json().await.map_err(|error| {
            KokuError::RateSource(format!("invalid frankfurter payload: {error}"))
        })?;
        parse_frankfurter(payload, from, to)
    }

    async fn fetch_currency_api(&self, from: &str, to: &str) -> Result<RateQuote> {
        let url = format!("{}/{}.json", self.currency_api_base, from.to_lowercase());
        let response = self.client.get(&url).send().await.map_err(|error| {
            KokuError::RateSource(format!("currency-api request failed: {error}"))
        })?;
        if !response.status().is_success() {
            return Err(KokuError::RateSource(format!(
                "currency-api returned HTTP {}",
                response.status()
            )));
        }
        let payload: CurrencyApiResponse = response.json().await.map_err(|error| {
            KokuError::RateSource(format!("invalid currency-api payload: {error}"))
        })?;
        parse_currency_api(payload, from, to)
    }
}

#[derive(Debug, Deserialize)]
struct FrankfurterResponse {
    date: String,
    rates: HashMap<String, f64>,
}

/// 解析 Frankfurter 响应为 `RateQuote`（纯函数，便于单测）。
fn parse_frankfurter(payload: FrankfurterResponse, from: &str, to: &str) -> Result<RateQuote> {
    let rate = payload
        .rates
        .get(to)
        .and_then(|value| Decimal::from_f64(*value))
        .ok_or_else(|| {
            KokuError::RateSource(format!("rate for {to} missing from source payload"))
        })?;
    if rate <= Decimal::ZERO {
        return Err(KokuError::RateSource(
            "exchange rate must be positive".to_owned(),
        ));
    }
    Ok(RateQuote {
        from: from.to_owned(),
        to: to.to_owned(),
        rate,
        date: payload.date,
        source: "frankfurter".to_owned(),
        stale: false,
    })
}

#[derive(Debug, Deserialize)]
struct CurrencyApiResponse {
    date: String,
    #[serde(flatten)]
    currencies: HashMap<String, HashMap<String, f64>>,
}

/// 解析 fawazahmed0/currency-api 响应为 `RateQuote`（纯函数，便于单测）。
/// 响应形如 `{"date":"2026-08-15","usd":{"cny":7.14,...}}`（键均为小写）。
fn parse_currency_api(payload: CurrencyApiResponse, from: &str, to: &str) -> Result<RateQuote> {
    let rate = payload
        .currencies
        .get(&from.to_lowercase())
        .and_then(|quotes| quotes.get(&to.to_lowercase()))
        .and_then(|value| Decimal::from_f64(*value))
        .ok_or_else(|| {
            KokuError::RateSource(format!("rate for {to} missing from currency-api payload"))
        })?;
    if rate <= Decimal::ZERO {
        return Err(KokuError::RateSource(
            "exchange rate must be positive".to_owned(),
        ));
    }
    Ok(RateQuote {
        from: from.to_owned(),
        to: to.to_owned(),
        rate,
        date: payload.date,
        source: "currency-api".to_owned(),
        stale: false,
    })
}

/// 缓存是否仍然新鲜：ECB 仅在交易日更新，周末/节假日沿用最近一个工作日的汇率，
/// 因此只要缓存日期与今天相差不超过 4 天就认为可复用。
pub fn rate_is_fresh(date: &str, today: NaiveDate) -> bool {
    let Ok(rate_date) = NaiveDate::parse_from_str(date, "%Y-%m-%d") else {
        return false;
    };
    let age = (today - rate_date).num_days();
    (0..=4).contains(&age)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frankfurter_payload_is_parsed_into_a_rate_quote() {
        let payload: FrankfurterResponse = serde_json::from_str(
            r#"{"amount":1.0,"base":"USD","date":"2026-08-14","rates":{"CNY":7.1445}}"#,
        )
        .unwrap();
        let quote = parse_frankfurter(payload, "USD", "CNY").unwrap();
        assert_eq!(quote.rate, Decimal::from_str_exact("7.1445").unwrap());
        assert_eq!(quote.date, "2026-08-14");
        assert_eq!(quote.source, "frankfurter");
        assert!(!quote.stale);
    }

    #[test]
    fn frankfurter_missing_quote_currency_is_rejected() {
        let payload: FrankfurterResponse =
            serde_json::from_str(r#"{"amount":1.0,"base":"USD","date":"2026-08-14","rates":{}}"#)
                .unwrap();
        assert!(parse_frankfurter(payload, "USD", "CNY").is_err());
    }

    #[test]
    fn currency_api_payload_is_parsed_with_lowercase_keys() {
        let payload: CurrencyApiResponse =
            serde_json::from_str(r#"{"date":"2026-08-15","usd":{"cny":7.1445,"eur":0.8645}}"#)
                .unwrap();
        let quote = parse_currency_api(payload, "USD", "CNY").unwrap();
        assert_eq!(quote.rate, Decimal::from_str_exact("7.1445").unwrap());
        assert_eq!(quote.date, "2026-08-15");
        assert_eq!(quote.source, "currency-api");
    }

    #[test]
    fn currency_api_missing_quote_currency_is_rejected() {
        let payload: CurrencyApiResponse =
            serde_json::from_str(r#"{"date":"2026-08-15","usd":{}}"#).unwrap();
        assert!(parse_currency_api(payload, "USD", "CNY").is_err());
    }

    #[test]
    fn rate_freshness_accepts_recent_weekday_rates_and_rejects_old_ones() {
        let today = NaiveDate::from_ymd_opt(2026, 8, 17).unwrap(); // 周一
        assert!(rate_is_fresh("2026-08-14", today)); // 上周五
        assert!(rate_is_fresh("2026-08-17", today));
        assert!(!rate_is_fresh("2026-08-10", today)); // 超过 4 天
        assert!(!rate_is_fresh("not-a-date", today));
    }
}
