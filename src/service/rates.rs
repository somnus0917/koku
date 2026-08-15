//! 汇率缓存：exchange_rates 表读写，按 (base, quote, date) 去重。

use super::*;
use crate::domain::RateQuote;
use crate::error::Result;
use crate::service::BookkeepingService;

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
}
