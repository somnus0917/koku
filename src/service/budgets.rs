//! 预算：按（分类, 年, 月）设置月度支出上限，并把上限回填到月度汇总。

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Utc};
use rusqlite::{params, OptionalExtension};
use rust_decimal::Decimal;

use super::*;
use crate::domain::{Budget, CategoryKind};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

impl BookkeepingService {
    /// 设置某分类某月的预算上限（覆盖已存在的同月上限）。仅支出分类可设预算。
    pub fn set_budget(
        &mut self,
        category_id: i64,
        year: i32,
        month: u32,
        limit_amount: Decimal,
    ) -> Result<Budget> {
        positive_amount(limit_amount)?;
        let category = self.category(category_id)?;
        if category.kind != CategoryKind::Expense {
            return Err(KokuError::InvalidInput(
                "budgets can only be set for expense categories".to_owned(),
            ));
        }
        self.conn.execute(
            "INSERT INTO budgets(category_id, year, month, limit_amount, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(category_id, year, month)
             DO UPDATE SET limit_amount = excluded.limit_amount",
            params![
                category_id,
                year,
                month,
                decimal_to_db(limit_amount),
                timestamp(Utc::now())
            ],
        )?;
        self.budget(category_id, year, month)
    }

    /// 清除某分类某月的预算；不存在时返回 NotFound。
    pub fn clear_budget(&mut self, category_id: i64, year: i32, month: u32) -> Result<Budget> {
        let budget = self.budget(category_id, year, month)?;
        self.conn.execute(
            "DELETE FROM budgets WHERE category_id = ?1 AND year = ?2 AND month = ?3",
            params![category_id, year, month],
        )?;
        Ok(budget)
    }

    pub fn budget(&self, category_id: i64, year: i32, month: u32) -> Result<Budget> {
        let row = self
            .conn
            .query_row(
                "SELECT b.category_id, c.name, c.kind, b.year, b.month, b.limit_amount
                 FROM budgets b JOIN categories c ON c.id = b.category_id
                 WHERE b.category_id = ?1 AND b.year = ?2 AND b.month = ?3",
                params![category_id, year, month],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i32>(3)?,
                        row.get::<_, u32>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?
            .ok_or(KokuError::NotFound {
                entity: "budget",
                id: category_id,
            })?;
        budget_from_row(row)
    }

    /// 某月已设置的全部预算（含已归档分类，历史预算仍然有效）。
    pub fn budgets(&self, year: i32, month: u32) -> Result<Vec<Budget>> {
        let mut statement = self.conn.prepare(
            "SELECT b.category_id, c.name, c.kind, b.year, b.month, b.limit_amount
             FROM budgets b JOIN categories c ON c.id = b.category_id
             WHERE b.year = ?1 AND b.month = ?2
             ORDER BY c.name",
        )?;
        let rows = statement.query_map(params![year, month], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i32>(3)?,
                row.get::<_, u32>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        rows.map(|row| budget_from_row(row?)).collect()
    }

    /// 把 `from` 月的预算整体复制到 `to` 月（同分类覆盖），返回复制的条数。
    pub fn copy_budgets(
        &mut self,
        from_year: i32,
        from_month: u32,
        to_year: i32,
        to_month: u32,
    ) -> Result<usize> {
        month_bounds(from_year, from_month)?;
        month_bounds(to_year, to_month)?;
        let source = self.budgets(from_year, from_month)?;
        let tx = self.conn.transaction()?;
        let mut copied = 0_usize;
        for budget in source {
            tx.execute(
                "INSERT INTO budgets(category_id, year, month, limit_amount, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(category_id, year, month)
                 DO UPDATE SET limit_amount = excluded.limit_amount",
                params![
                    budget.category_id,
                    to_year,
                    to_month,
                    decimal_to_db(budget.limit_amount),
                    timestamp(Utc::now())
                ],
            )?;
            copied += 1;
        }
        tx.commit()?;
        Ok(copied)
    }

    /// 每月首次访问时把上月的预算自动带入本月（每个自然月只执行一次，幂等）。
    pub fn rollover_budgets_once(&mut self, now: DateTime<Utc>) -> Result<usize> {
        let year = now.year();
        let month = now.month();
        let key = format!("budget_rollover:{year:04}-{month:02}");
        if self.get_setting(&key)?.is_some() {
            return Ok(0);
        }
        let (last_year, last_month) = if month == 1 {
            (year - 1, 12)
        } else {
            (year, month - 1)
        };
        let copied = self.copy_budgets(last_year, last_month, year, month)?;
        self.set_setting(&key, "done")?;
        Ok(copied)
    }

    /// 某月各支出分类的预算上限映射（供月度汇总回填）。
    pub(crate) fn budget_limits(&self, year: i32, month: u32) -> Result<BTreeMap<i64, Decimal>> {
        let mut statement = self.conn.prepare(
            "SELECT category_id, limit_amount FROM budgets WHERE year = ?1 AND month = ?2",
        )?;
        let rows = statement.query_map(params![year, month], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut limits = BTreeMap::new();
        for row in rows {
            let (category_id, limit) = row?;
            limits.insert(category_id, decimal_from_db(&limit)?);
        }
        Ok(limits)
    }
}

fn budget_from_row(row: (i64, String, String, i32, u32, String)) -> Result<Budget> {
    Ok(Budget {
        category_id: row.0,
        category_name: row.1,
        category_kind: CategoryKind::from_db(&row.2)?,
        year: row.3,
        month: row.4,
        limit_amount: decimal_from_db(&row.5)?,
    })
}
