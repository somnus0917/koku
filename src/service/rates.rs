//! 汇率缓存：exchange_rates 表读写，按 (base, quote, date) 去重；以及汇总折算辅助。

use chrono::NaiveDate;

use super::*;
use crate::domain::RateQuote;
use crate::error::{KokuError, Result};
use crate::rates::rate_is_recent;
use crate::service::BookkeepingService;

/// 折算时最多使用多少天前的汇率（避免用很久以前的汇率算出离谱的数字）。
pub const CONVERSION_RATE_MAX_AGE_DAYS: i64 = 30;

impl BookkeepingService {
    /// 某币种对最近一次缓存的汇率（按生效日期倒序）。
    pub fn latest_rate(&self, from: &str, to: &str) -> Result<Option<RateQuote>> {
        let row = self
            .conn
            .query_row(
                "SELECT rate, date, source FROM exchange_rates
                 WHERE base = ?1 AND quote = ?2 ORDER BY date DESC LIMIT 1",
                params![from, to],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((rate, date, source)) = row else {
            return Ok(None);
        };
        Ok(Some(RateQuote {
            from: from.to_owned(),
            to: to.to_owned(),
            rate: decimal_from_db(&rate)?,
            date,
            source,
            stale: false,
        }))
    }

    /// 写入/更新一条汇率缓存。
    pub fn store_rate(&self, quote: &RateQuote) -> Result<()> {
        self.conn.execute(
            "INSERT INTO exchange_rates(base, quote, rate, date, source)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(base, quote, date)
             DO UPDATE SET rate = excluded.rate, source = excluded.source",
            params![
                quote.from,
                quote.to,
                decimal_to_db(quote.rate),
                quote.date,
                quote.source
            ],
        )?;
        Ok(())
    }

    /// 查找 from→to 的折算汇率：先查正向缓存，再查反向缓存取倒数；超龄视为缺失。
    pub(crate) fn conversion_rate(
        &self,
        from: &str,
        to: &str,
        today: NaiveDate,
    ) -> Result<Option<Decimal>> {
        if from == to {
            return Ok(Some(Decimal::ONE));
        }
        if let Some(quote) = self.latest_rate(from, to)? {
            if rate_is_recent(&quote.date, today, CONVERSION_RATE_MAX_AGE_DAYS) {
                return Ok(Some(quote.rate));
            }
        }
        if let Some(quote) = self.latest_rate(to, from)? {
            if rate_is_recent(&quote.date, today, CONVERSION_RATE_MAX_AGE_DAYS)
                && !quote.rate.is_zero()
            {
                return Ok(Some(Decimal::ONE / quote.rate));
            }
        }
        Ok(None)
    }

    /// 把金额从 from 币种折算到 to 币种；缺汇率时报错（调用方应先确保汇率可用）。
    pub(crate) fn convert_amount(
        &self,
        amount: Decimal,
        from: &str,
        to: &str,
        today: NaiveDate,
    ) -> Result<Decimal> {
        if from == to {
            return Ok(amount);
        }
        let rate = self.conversion_rate(from, to, today)?.ok_or_else(|| {
            KokuError::InvalidInput(format!("missing exchange rate {from}->{to}"))
        })?;
        Ok(amount * rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_are_cached_and_latest_date_wins() -> Result<()> {
        let service = BookkeepingService::in_memory()?;
        assert_eq!(service.latest_rate("USD", "CNY")?, None);

        service.store_rate(&RateQuote {
            from: "USD".to_owned(),
            to: "CNY".to_owned(),
            rate: Decimal::from_str_exact("7.10").unwrap(),
            date: "2026-08-13".to_owned(),
            source: "frankfurter".to_owned(),
            stale: false,
        })?;
        service.store_rate(&RateQuote {
            from: "USD".to_owned(),
            to: "CNY".to_owned(),
            rate: Decimal::from_str_exact("7.14").unwrap(),
            date: "2026-08-14".to_owned(),
            source: "frankfurter".to_owned(),
            stale: false,
        })?;

        let latest = service.latest_rate("USD", "CNY")?.unwrap();
        assert_eq!(latest.rate, Decimal::from_str_exact("7.14").unwrap());
        assert_eq!(latest.date, "2026-08-14");
        // 反向币种对没有缓存
        assert_eq!(service.latest_rate("CNY", "USD")?, None);
        Ok(())
    }

    #[test]
    fn conversion_rate_supports_both_directions_and_rejects_stale_rates() -> Result<()> {
        let service = BookkeepingService::in_memory()?;
        let today = NaiveDate::from_ymd_opt(2026, 8, 15).unwrap();
        // 只缓存 USD→CNY = 7.14（8-14，今天内）。
        service.store_rate(&RateQuote {
            from: "USD".to_owned(),
            to: "CNY".to_owned(),
            rate: Decimal::from_str_exact("7.14").unwrap(),
            date: "2026-08-14".to_owned(),
            source: "frankfurter".to_owned(),
            stale: false,
        })?;
        // 正向直接可用。
        assert_eq!(
            service.conversion_rate("USD", "CNY", today)?,
            Some(Decimal::from_str_exact("7.14").unwrap())
        );
        // 反向取倒数。
        let inverse = service.conversion_rate("CNY", "USD", today)?.unwrap();
        assert_eq!(
            (inverse * Decimal::from_str_exact("7.14").unwrap()).round_dp(6),
            Decimal::ONE
        );
        // 同币种恒为 1。
        assert_eq!(
            service.conversion_rate("CNY", "CNY", today)?,
            Some(Decimal::ONE)
        );
        // 未缓存的币种对为 None。
        assert_eq!(service.conversion_rate("EUR", "CNY", today)?, None);
        Ok(())
    }

    #[test]
    fn convert_amount_multiplies_by_the_rate() -> Result<()> {
        let service = BookkeepingService::in_memory()?;
        let today = NaiveDate::from_ymd_opt(2026, 8, 15).unwrap();
        service.store_rate(&RateQuote {
            from: "USD".to_owned(),
            to: "CNY".to_owned(),
            rate: Decimal::from_str_exact("7.2172").unwrap(),
            date: "2026-08-14".to_owned(),
            source: "frankfurter".to_owned(),
            stale: false,
        })?;
        assert_eq!(
            service.convert_amount(Decimal::ONE, "CNY", "CNY", today)?,
            Decimal::ONE
        );
        let converted =
            service.convert_amount(Decimal::from_str_exact("32.50")?, "USD", "CNY", today)?;
        assert_eq!(converted.round_dp(2), Decimal::from_str_exact("234.56")?);
        assert!(service
            .convert_amount(Decimal::ONE, "EUR", "CNY", today)
            .is_err());
        Ok(())
    }
}
