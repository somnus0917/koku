//! 净资产历史快照：每日、每显示币种最多一条，当日重复采集会幂等刷新。

use chrono::{Duration, NaiveDate, Utc};
use rusqlite::params;

use super::*;
use crate::domain::NetWorthSnapshot;
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

impl BookkeepingService {
    /// 保存指定日期的当前净资产；同日同币种重复执行会更新为最新值。
    pub fn save_net_worth_snapshot(
        &mut self,
        currency: &str,
        snapshot_date: NaiveDate,
    ) -> Result<NetWorthSnapshot> {
        let summary = self.balance_summary(currency)?;
        let created_at = Utc::now();
        self.conn.execute(
            "INSERT INTO net_worth_snapshots(
                 snapshot_date, currency, total_assets, total_liabilities, net_worth, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(snapshot_date, currency) DO UPDATE SET
                 total_assets = excluded.total_assets,
                 total_liabilities = excluded.total_liabilities,
                 net_worth = excluded.net_worth,
                 created_at = excluded.created_at",
            params![
                snapshot_date.to_string(),
                summary.currency,
                decimal_to_db(summary.total_assets),
                decimal_to_db(summary.total_liabilities),
                decimal_to_db(summary.net_worth),
                timestamp(created_at),
            ],
        )?;
        Ok(NetWorthSnapshot {
            snapshot_date,
            currency: summary.currency,
            total_assets: summary.total_assets,
            total_liabilities: summary.total_liabilities,
            net_worth: summary.net_worth,
            created_at,
        })
    }

    /// 返回最近 `days` 天内的快照，按日期升序；最多支持十年。
    pub fn net_worth_snapshots(&self, currency: &str, days: u32) -> Result<Vec<NetWorthSnapshot>> {
        if !(1..=3650).contains(&days) {
            return Err(KokuError::InvalidInput(
                "net worth snapshot days must be 1..3650".to_owned(),
            ));
        }
        let currency = normalize_currency(currency.to_owned())?;
        let start = Utc::now().date_naive() - Duration::days(i64::from(days - 1));
        let mut statement = self.conn.prepare(
            "SELECT snapshot_date, currency, total_assets, total_liabilities, net_worth, created_at
             FROM net_worth_snapshots
             WHERE currency = ?1 AND snapshot_date >= ?2
             ORDER BY snapshot_date",
        )?;
        let rows = statement.query_map(params![currency, start.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (snapshot_date, currency, assets, liabilities, net_worth, created_at) = row?;
            Ok(NetWorthSnapshot {
                snapshot_date: NaiveDate::parse_from_str(&snapshot_date, "%Y-%m-%d").map_err(
                    |error| KokuError::InvalidInput(format!("invalid snapshot date: {error}")),
                )?,
                currency,
                total_assets: decimal_from_db(&assets)?,
                total_liabilities: decimal_from_db(&liabilities)?,
                net_worth: decimal_from_db(&net_worth)?,
                created_at: parse_timestamp(&created_at)?,
            })
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::AccountType;
    use rust_decimal::Decimal;

    #[test]
    fn daily_snapshot_upserts_and_lists_in_order() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        service.create_account("现金", AccountType::Cash, "CNY", Decimal::from(100_u32))?;
        let today = Utc::now().date_naive();
        service.save_net_worth_snapshot("CNY", today)?;
        service.create_account("储蓄", AccountType::Savings, "CNY", Decimal::from(50_u32))?;
        service.save_net_worth_snapshot("CNY", today)?;

        let snapshots = service.net_worth_snapshots("CNY", 365)?;
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].snapshot_date, today);
        assert_eq!(snapshots[0].total_assets, Decimal::from(150_u32));
        assert_eq!(snapshots[0].net_worth, Decimal::from(150_u32));
        Ok(())
    }

    #[test]
    fn snapshot_window_is_validated() -> Result<()> {
        let service = BookkeepingService::in_memory()?;
        assert!(service.net_worth_snapshots("CNY", 0).is_err());
        assert!(service.net_worth_snapshots("CNY", 3651).is_err());
        Ok(())
    }
}
