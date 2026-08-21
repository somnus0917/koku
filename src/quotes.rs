//! 持仓市价客户端：按股票代码识别市场，优先 Stooq、失败后回退 Yahoo Finance。
//! 请求共享节流与有限重试；取价失败时调用方保留上一次有效市价。

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Datelike, Timelike, Utc, Weekday};
use chrono_tz::{America::New_York, Asia::Hong_Kong, Asia::Shanghai};
use rust_decimal::Decimal;
use serde::Serialize;
use tokio::sync::Mutex;
use tokio::time::{sleep, Instant};

use crate::error::{KokuError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Market {
    Us,
    Hk,
    CnSh,
    CnSz,
    CnStar,
    Unknown,
}

impl Market {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Us => "us",
            Self::Hk => "hk",
            Self::CnSh => "cn_sh",
            Self::CnSz => "cn_sz",
            Self::CnStar => "cn_star",
            Self::Unknown => "unknown",
        }
    }
    pub fn from_db(value: &str) -> Self {
        match value {
            "us" => Self::Us,
            "hk" => Self::Hk,
            "cn_sh" => Self::CnSh,
            "cn_sz" => Self::CnSz,
            "cn_star" => Self::CnStar,
            _ => Self::Unknown,
        }
    }
    /// 以交易所当地时区判断常规收盘，纽约时区会自动处理夏令时。
    pub fn close_has_passed(self, now: DateTime<Utc>) -> bool {
        match self {
            Self::CnSh | Self::CnSz | Self::CnStar => {
                close_in_local_time(now.with_timezone(&Shanghai), 15, 10)
            }
            Self::Hk => close_in_local_time(now.with_timezone(&Hong_Kong), 16, 10),
            Self::Us => close_in_local_time(now.with_timezone(&New_York), 16, 10),
            Self::Unknown => false,
        }
    }
}

fn close_in_local_time<Tz: chrono::TimeZone>(now: DateTime<Tz>, hour: u32, minute: u32) -> bool {
    !matches!(now.weekday(), Weekday::Sat | Weekday::Sun)
        && (now.hour(), now.minute()) >= (hour, minute)
}

/// 收盘后的自动刷新只做一次；非可识别市场由用户通过手动价格兜底。
pub fn should_refresh_after_close(
    market: &str,
    updated_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    let market = Market::from_db(market);
    market.close_has_passed(now)
        && updated_at
            .map(|updated| updated.date_naive() != now.date_naive())
            .unwrap_or(true)
}

/// 识别带后缀代码以及常用裸代码。六位 68 开头为科创板，6/9 为沪市，0/3 为深市。
pub fn detect_market(symbol: &str) -> Market {
    let symbol = symbol.trim().to_ascii_uppercase();
    if symbol.ends_with(".US") {
        return Market::Us;
    }
    if symbol.ends_with(".HK") {
        return Market::Hk;
    }
    if symbol.ends_with(".SS") || symbol.ends_with(".SH") {
        return if symbol.starts_with("68") {
            Market::CnStar
        } else {
            Market::CnSh
        };
    }
    if symbol.ends_with(".SZ") {
        return Market::CnSz;
    }
    if symbol.len() == 6 && symbol.chars().all(|ch| ch.is_ascii_digit()) {
        return if symbol.starts_with("68") {
            Market::CnStar
        } else if symbol.starts_with('6') || symbol.starts_with('9') {
            Market::CnSh
        } else if symbol.starts_with('0') || symbol.starts_with('3') {
            Market::CnSz
        } else {
            Market::Unknown
        };
    }
    if (4..=5).contains(&symbol.len()) && symbol.chars().all(|ch| ch.is_ascii_digit()) {
        return Market::Hk;
    }
    if !symbol.is_empty()
        && symbol
            .chars()
            .all(|ch| ch.is_ascii_alphabetic() || ch == '.' || ch == '-')
    {
        Market::Us
    } else {
        Market::Unknown
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Quote {
    pub symbol: String,
    pub price: Decimal,
    pub date: String,
    pub source: String,
    pub market: Market,
}

#[derive(Debug, Clone)]
pub struct QuoteClient {
    client: reqwest::Client,
    stooq_base: String,
    yahoo_base: String,
    nasdaq_base: String,
    next_request_at: Arc<Mutex<Instant>>,
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
            stooq_base: "https://stooq.com/q/l".to_owned(),
            yahoo_base: "https://query1.finance.yahoo.com/v8/finance/chart".to_owned(),
            nasdaq_base: "https://api.nasdaq.com/api/quote".to_owned(),
            next_request_at: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// 每轮先 Stooq；美股再尝试 Nasdaq，随后回退 Yahoo Finance，最多三轮。
    pub async fn fetch(&self, symbol: &str) -> Result<Quote> {
        let market = detect_market(symbol);
        let stooq_symbol = stooq_symbol(symbol, market);
        let yahoo_symbol = yahoo_symbol(symbol, market);
        let mut errors = Vec::new();
        for attempt in 0..3 {
            match self.fetch_stooq(symbol, &stooq_symbol, market).await {
                Ok(quote) => return Ok(quote),
                Err(error) => errors.push(format!("stooq: {error}")),
            }
            if market == Market::Us {
                match self.fetch_nasdaq(symbol, market).await {
                    Ok(quote) => return Ok(quote),
                    Err(error) => errors.push(format!("nasdaq: {error}")),
                }
            }
            match self.fetch_yahoo(symbol, &yahoo_symbol, market).await {
                Ok(quote) => return Ok(quote),
                Err(error) => errors.push(format!("yahoo: {error}")),
            }
            if attempt < 2 {
                sleep(Duration::from_millis(300 * (attempt + 1) as u64)).await;
            }
        }
        Err(KokuError::RateSource(format!(
            "no quote available for {symbol}; {}",
            errors.join("; ")
        )))
    }

    async fn fetch_stooq(
        &self,
        requested: &str,
        provider_symbol: &str,
        market: Market,
    ) -> Result<Quote> {
        self.throttle().await;
        let url = format!(
            "{}?s={provider_symbol}&f=sd2t2ohlcv&h&e=csv",
            self.stooq_base
        );
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
        parse_stooq_csv(&text, requested, market)
    }

    async fn fetch_yahoo(
        &self,
        requested: &str,
        provider_symbol: &str,
        market: Market,
    ) -> Result<Quote> {
        self.throttle().await;
        let url = format!("{}/{provider_symbol}?range=5d&interval=1d", self.yahoo_base);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|error| KokuError::RateSource(format!("yahoo request failed: {error}")))?;
        if !response.status().is_success() {
            return Err(KokuError::RateSource(format!(
                "yahoo returned HTTP {}",
                response.status()
            )));
        }
        let payload: serde_json::Value = response.json().await.map_err(|error| {
            KokuError::RateSource(format!("could not read yahoo payload: {error}"))
        })?;
        parse_yahoo_payload(&payload, requested, market)
    }

    async fn fetch_nasdaq(&self, requested: &str, market: Market) -> Result<Quote> {
        self.throttle().await;
        let code = provider_code(requested);
        let url = format!("{}/{code}/info?assetclass=stocks", self.nasdaq_base);
        let response = self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|error| KokuError::RateSource(format!("nasdaq request failed: {error}")))?;
        if !response.status().is_success() {
            return Err(KokuError::RateSource(format!(
                "nasdaq returned HTTP {}",
                response.status()
            )));
        }
        let payload: serde_json::Value = response.json().await.map_err(|error| {
            KokuError::RateSource(format!("could not read nasdaq payload: {error}"))
        })?;
        parse_nasdaq_payload(&payload, requested, market)
    }

    async fn throttle(&self) {
        let mut next = self.next_request_at.lock().await;
        let now = Instant::now();
        if *next > now {
            sleep(*next - now).await;
        }
        *next = Instant::now() + Duration::from_millis(250);
    }
}

fn provider_code(symbol: &str) -> String {
    symbol
        .trim()
        .to_ascii_uppercase()
        .split('.')
        .next()
        .unwrap_or_default()
        .to_owned()
}
fn stooq_symbol(symbol: &str, market: Market) -> String {
    let code = provider_code(symbol);
    match market {
        Market::Us => format!("{code}.US"),
        Market::Hk => format!("{code:0>4}.HK"),
        Market::CnSh | Market::CnStar => format!("{code}.SS"),
        Market::CnSz => format!("{code}.SZ"),
        Market::Unknown => symbol.trim().to_ascii_lowercase(),
    }
    .to_ascii_lowercase()
}
fn yahoo_symbol(symbol: &str, market: Market) -> String {
    let code = provider_code(symbol);
    match market {
        Market::Us => code,
        Market::Hk => format!("{code:0>4}.HK"),
        Market::CnSh | Market::CnStar => format!("{code}.SS"),
        Market::CnSz => format!("{code}.SZ"),
        Market::Unknown => symbol.trim().to_ascii_uppercase(),
    }
}

fn parse_stooq_csv(text: &str, requested_symbol: &str, market: Market) -> Result<Quote> {
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
        symbol: requested_symbol.to_owned(),
        price,
        date: fields[1].to_owned(),
        source: "stooq".to_owned(),
        market,
    })
}

fn parse_yahoo_payload(
    payload: &serde_json::Value,
    requested_symbol: &str,
    market: Market,
) -> Result<Quote> {
    let meta = payload
        .pointer("/chart/result/0/meta")
        .ok_or_else(|| KokuError::RateSource("yahoo returned no quote metadata".to_owned()))?;
    let raw_price = meta
        .get("regularMarketPrice")
        .and_then(|value| value.as_f64())
        .ok_or_else(|| KokuError::RateSource("yahoo returned no market price".to_owned()))?;
    let price = Decimal::from_str(&raw_price.to_string())
        .map_err(|error| KokuError::RateSource(format!("invalid yahoo price: {error}")))?;
    if price <= Decimal::ZERO {
        return Err(KokuError::RateSource(
            "yahoo price must be positive".to_owned(),
        ));
    }
    let timestamp = meta
        .get("regularMarketTime")
        .and_then(|value| value.as_i64())
        .unwrap_or_else(|| Utc::now().timestamp());
    let date = DateTime::from_timestamp(timestamp, 0)
        .unwrap_or_else(Utc::now)
        .date_naive()
        .to_string();
    Ok(Quote {
        symbol: requested_symbol.to_owned(),
        price,
        date,
        source: "yahoo_finance".to_owned(),
        market,
    })
}

fn parse_nasdaq_payload(
    payload: &serde_json::Value,
    requested_symbol: &str,
    market: Market,
) -> Result<Quote> {
    let value = payload
        .pointer("/data/primaryData/lastSalePrice")
        .and_then(|value| value.as_str())
        .ok_or_else(|| KokuError::RateSource("nasdaq returned no market price".to_owned()))?;
    let normalized = value.trim().trim_start_matches('$').replace(',', "");
    let price = Decimal::from_str(&normalized).map_err(|error| {
        KokuError::RateSource(format!("invalid nasdaq price {value:?}: {error}"))
    })?;
    if price <= Decimal::ZERO {
        return Err(KokuError::RateSource(
            "nasdaq price must be positive".to_owned(),
        ));
    }
    Ok(Quote {
        symbol: requested_symbol.to_owned(),
        price,
        date: Utc::now().date_naive().to_string(),
        source: "nasdaq".to_owned(),
        market,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_mainland_star_hk_and_us_symbols() {
        assert_eq!(detect_market("600519"), Market::CnSh);
        assert_eq!(detect_market("688981"), Market::CnStar);
        assert_eq!(detect_market("000001"), Market::CnSz);
        assert_eq!(detect_market("0700"), Market::Hk);
        assert_eq!(detect_market("AAPL"), Market::Us);
        assert_eq!(stooq_symbol("AAPL", Market::Us), "aapl.us");
        assert_eq!(yahoo_symbol("0700", Market::Hk), "0700.HK");
    }
    #[test]
    fn parses_a_normal_stooq_csv_response() {
        let csv = "Symbol,Date,Time,Open,High,Low,Close,Volume\nAAPL.US,2026-08-14,16:00:00,172.75,174.30,172.50,173.00,65786300\n";
        let quote = parse_stooq_csv(csv, "AAPL", Market::Us).unwrap();
        assert_eq!(quote.symbol, "AAPL");
        assert_eq!(quote.price, Decimal::from_str_exact("173.00").unwrap());
        assert_eq!(quote.source, "stooq");
    }
    #[test]
    fn parses_yahoo_market_quote() {
        let payload = serde_json::json!({"chart":{"result":[{"meta":{"regularMarketPrice":173.0,"regularMarketTime":1786732800}}]}});
        let quote = parse_yahoo_payload(&payload, "AAPL", Market::Us).unwrap();
        assert_eq!(quote.price, Decimal::from(173_u32));
        assert_eq!(quote.source, "yahoo_finance");
    }

    #[test]
    fn parses_nasdaq_us_quote() {
        let payload = serde_json::json!({"data":{"primaryData":{"lastSalePrice":"$215.18"}}});
        let quote = parse_nasdaq_payload(&payload, "NVDA", Market::Us).unwrap();
        assert_eq!(quote.price, Decimal::from_str_exact("215.18").unwrap());
        assert_eq!(quote.source, "nasdaq");
    }

    #[test]
    fn only_refreshes_recognized_markets_once_after_close() {
        let after_asia_close = chrono::DateTime::parse_from_rfc3339("2026-08-21T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(should_refresh_after_close(
            "cn_star",
            None,
            after_asia_close
        ));
        assert!(!should_refresh_after_close("us", None, after_asia_close));
        assert!(!should_refresh_after_close(
            "unknown",
            None,
            after_asia_close
        ));
        let after_us_close = chrono::DateTime::parse_from_rfc3339("2026-08-21T20:15:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(should_refresh_after_close("us", None, after_us_close));
        assert!(!should_refresh_after_close(
            "hk",
            Some(after_asia_close),
            after_asia_close
        ));
    }
}
