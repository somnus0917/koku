//! 持仓市价客户端：从 Stooq 拉取免费 CSV 行情（无需 key），用于持仓市价自动更新。
//!
//! Stooq 接口：`https://stooq.com/q/l/?s=<SYMBOL>&f=sd2t2ohlcv&h&e=csv`，
//! 返回两行 CSV：表头 `Symbol,Date,Time,Open,High,Low,Close,Volume` 与数据行；
//! 无数据时 Close 为 `N/D`。市场后缀由用户负责（如 `AAPL.US`、`600519.SS`、
//! `0700.HK`）；本模块只做「拉取 + 解析」，缓存/新鲜度判断在 API 层与前端。

use std::str::FromStr;
use std::time::Duration;

use rust_decimal::Decimal;
use serde::Serialize;

use crate::error::{KokuError, Result};

/// 一条市价快照。
#[derive(Debug, Clone, Serialize)]
pub struct Quote {
    pub symbol: String,
    pub price: Decimal,
    /// 行情日期（YYYY-MM-DD）。
    pub date: String,
    pub source: String,
}

/// 共享的行情 HTTP 客户端（内部复用连接，超时与汇率客户端一致）。
#[derive(Debug, Clone)]
pub struct QuoteClient {
    client: reqwest::Client,
    base: String,
}

impl Default for QuoteClient {
    fn default() -> Self {
        Self::new()
    }
}

impl QuoteClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(10))
                .user_agent("koku/0.1 (personal ledger)")
                .build()
                .expect("failed to build quote http client"),
            base: "https://stooq.com/q/l".to_owned(),
        }
    }

    /// 拉取某标的的最新收盘价。
    pub async fn fetch(&self, symbol: &str) -> Result<Quote> {
        let url = format!("{}?s={symbol}&f=sd2t2ohlcv&h&e=csv", self.base);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|error| KokuError::RateSource(format!("stooq request failed: {error}")))?;
        if !response.status().is_success() {
            return Err(KokuError::RateSource(format!(
                "stooq returned HTTP {}",
                response.status()
            )));
        }
        let text = response.text().await.map_err(|error| {
            KokuError::RateSource(format!("could not read stooq payload: {error}"))
        })?;
        parse_stooq_csv(&text, symbol)
    }
}

/// 解析 Stooq CSV 响应为 `Quote`（纯函数，便于单测）。
fn parse_stooq_csv(text: &str, requested_symbol: &str) -> Result<Quote> {
    let mut lines = text.lines();
    let header = lines.next().unwrap_or_default();
    if !header.to_ascii_uppercase().contains("SYMBOL") {
        return Err(KokuError::RateSource(
            "unexpected stooq payload (missing header)".to_owned(),
        ));
    }
    let data = lines
        .next()
        .ok_or_else(|| KokuError::RateSource("stooq returned no data row".to_owned()))?;
    let fields: Vec<&str> = data.split(',').map(str::trim).collect();
    if fields.len() < 7 {
        return Err(KokuError::RateSource(format!(
            "unexpected stooq row: {data}"
        )));
    }
    // 列顺序：Symbol,Date,Time,Open,High,Low,Close,Volume
    let close = fields[6];
    if close.is_empty() || close.eq_ignore_ascii_case("N/D") {
        return Err(KokuError::RateSource(format!(
            "no market data for {requested_symbol} on stooq"
        )));
    }
    let price = Decimal::from_str(close).map_err(|error| {
        KokuError::RateSource(format!("invalid stooq price {close:?}: {error}"))
    })?;
    if price <= Decimal::ZERO {
        return Err(KokuError::RateSource(
            "stooq price must be positive".to_owned(),
        ));
    }
    Ok(Quote {
        symbol: fields[0].to_owned(),
        price,
        date: fields[1].to_owned(),
        source: "stooq".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_normal_stooq_csv_response() {
        let csv = "Symbol,Date,Time,Open,High,Low,Close,Volume\nAAPL.US,2026-08-14,16:00:00,172.75,174.30,172.50,173.00,65786300\n";
        let quote = parse_stooq_csv(csv, "AAPL.US").unwrap();
        assert_eq!(quote.symbol, "AAPL.US");
        assert_eq!(quote.price, Decimal::from_str_exact("173.00").unwrap());
        assert_eq!(quote.date, "2026-08-14");
        assert_eq!(quote.source, "stooq");
    }

    #[test]
    fn rejects_no_data_and_garbage() {
        let no_data =
            "Symbol,Date,Time,Open,High,Low,Close,Volume\nAAPL.US,N/D,N/D,N/D,N/D,N/D,N/D,N/D\n";
        assert!(parse_stooq_csv(no_data, "AAPL.US").is_err());
        assert!(parse_stooq_csv("garbage", "X").is_err());
        let empty = "Symbol,Date,Time,Open,High,Low,Close,Volume\n";
        assert!(parse_stooq_csv(empty, "X").is_err());
    }
}
