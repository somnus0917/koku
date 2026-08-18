//! 交易拆分：把一笔 expense/income 的金额按多个分类归属。
//!
//! 父交易负责真实资金流（账户余额只动一次）；分类统计按拆分展开。
//! 仅支持 expense / income；拆分金额必须 > 0 且总和等于父交易金额。

use chrono::Utc;
use rusqlite::{params, OptionalExtension, TransactionBehavior};

use super::*;
use crate::domain::{CategoryKind, SplitInput, TransactionKind, TransactionSplit};
use crate::error::{KokuError, Result};

impl BookkeepingService {
    /// 列出交易的拆分；无拆分时返回空列表。
    pub fn list_transaction_splits(&self, transaction_id: i64) -> Result<Vec<TransactionSplit>> {
        let mut statement = self.conn.prepare(
            "SELECT id, transaction_id, category_id, amount, note, created_at
             FROM transaction_splits WHERE transaction_id = ?1 ORDER BY id",
        )?;
        let rows = statement.query_map([transaction_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        let mut result = Vec::new();
        for row in rows {
            let (id, transaction_id, category_id, amount, note, created_at) = row?;
            result.push(TransactionSplit {
                id,
                transaction_id,
                category_id,
                amount: decimal_from_db(&amount)?,
                note,
                created_at: parse_timestamp(&created_at)?,
            });
        }
        Ok(result)
    }

    /// 原子替换交易的拆分（空列表即清除，恢复父交易分类统计）。
    ///
    /// 校验顺序：交易存在 → 类型支持拆分 → 各拆分分类存在且类型匹配 →
    /// 金额 > 0 → 总和等于父交易金额 → 整体替换；任一步失败全部回滚。
    pub fn set_transaction_splits(
        &mut self,
        transaction_id: i64,
        splits: &[SplitInput],
    ) -> Result<Vec<TransactionSplit>> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let transaction = Self::transaction_in_tx(&tx, transaction_id)?;
        if !matches!(
            transaction.kind,
            TransactionKind::Expense | TransactionKind::Income
        ) {
            return Err(KokuError::InvalidInput(
                "only expense and income transactions can be split".to_owned(),
            ));
        }
        let expected_kind = match transaction.kind {
            TransactionKind::Expense => CategoryKind::Expense,
            TransactionKind::Income => CategoryKind::Income,
            _ => unreachable!("validated above"),
        };
        let mut total = Decimal::ZERO;
        for split in splits {
            if split.amount <= Decimal::ZERO {
                return Err(KokuError::InvalidInput(
                    "split amount must be greater than zero".to_owned(),
                ));
            }
            let category = Self::category_in_tx(&tx, split.category_id)?;
            if category.kind != expected_kind {
                return Err(KokuError::CategoryKindMismatch {
                    expected: expected_kind.as_str(),
                    actual: category.kind.as_str(),
                });
            }
            total += split.amount;
        }
        if total != transaction.amount {
            return Err(KokuError::InvalidInput(format!(
                "split amounts must sum to the transaction amount ({})",
                transaction.amount
            )));
        }
        // 原子替换旧拆分。
        tx.execute(
            "DELETE FROM transaction_splits WHERE transaction_id = ?1",
            [transaction_id],
        )?;
        let now = timestamp(Utc::now());
        for split in splits {
            tx.execute(
                "INSERT INTO transaction_splits(transaction_id, category_id, amount, note, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    transaction_id,
                    split.category_id,
                    decimal_to_db(split.amount),
                    split.note,
                    now
                ],
            )?;
        }
        tx.commit()?;
        self.list_transaction_splits(transaction_id)
    }

    /// 清除交易的拆分（等价于空列表）。
    pub fn clear_transaction_splits(&mut self, transaction_id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM transaction_splits WHERE transaction_id = ?1",
            [transaction_id],
        )?;
        Ok(())
    }

    /// 事务内：查询交易拆分金额总和；无拆分返回 `None`。
    pub(super) fn splits_total_in_tx(
        tx: &SqlTransaction<'_>,
        transaction_id: i64,
    ) -> Result<Option<Decimal>> {
        let total: Option<String> = tx
            .query_row(
                "SELECT COALESCE(CAST(SUM(amount) AS TEXT), '') FROM transaction_splits WHERE transaction_id = ?1",
                [transaction_id],
                |row| row.get(0),
            )
            .optional()?;
        match total {
            Some(value) if !value.is_empty() => Ok(Some(decimal_from_db(&value)?)),
            _ => Ok(None),
        }
    }

    /// 事务内：删除交易的拆分（永久删除交易时调用）。
    pub(super) fn delete_transaction_splits_in_tx(
        tx: &SqlTransaction<'_>,
        transaction_id: i64,
    ) -> Result<()> {
        tx.execute(
            "DELETE FROM transaction_splits WHERE transaction_id = ?1",
            [transaction_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::AccountType;

    /// 建一个带默认分类的账本，返回 (service, account, 餐饮, 购物, 工资) id。
    fn seeded() -> Result<(BookkeepingService, i64, i64, i64, i64)> {
        let mut service = BookkeepingService::in_memory()?;
        service.ensure_default_categories()?;
        let account =
            service.create_account("零钱", AccountType::Cash, "CNY", Decimal::from(10000_u32))?;
        let food = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "餐饮")
            .unwrap();
        let shopping = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "购物")
            .unwrap();
        let salary = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "工资")
            .unwrap();
        Ok((service, account.id, food.id, shopping.id, salary.id))
    }

    /// 固定测试时间（2026-08-15），保证统计测试落在 2026-08 月。
    fn fixed_time() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-08-15T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    fn input(category_id: i64, amount: &str) -> SplitInput {
        SplitInput {
            category_id,
            amount: Decimal::from_str_exact(amount).unwrap(),
            note: None,
        }
    }

    fn sum_amounts(splits: &[TransactionSplit]) -> Decimal {
        splits.iter().fold(Decimal::ZERO, |acc, s| acc + s.amount)
    }

    #[test]
    fn expense_can_have_multiple_splits() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx = service.record_expense(
            account,
            food,
            Decimal::from_str_exact("238.00").unwrap(),
            fixed_time(),
            "沃尔玛",
        )?;
        let splits = service.set_transaction_splits(
            tx.id,
            &[
                input(food, "80.00"),
                input(shopping, "108.00"),
                input(food, "50.00"),
            ],
        )?;
        assert_eq!(splits.len(), 3);
        assert_eq!(sum_amounts(&splits), tx.amount);
        Ok(())
    }

    #[test]
    fn income_can_have_multiple_splits() -> Result<()> {
        let (mut service, account, _, _, salary) = seeded()?;
        let interest = service
            .categories()?
            .into_iter()
            .find(|item| item.name == "利息")
            .unwrap();
        let tx = service.record_income(
            account,
            salary,
            Decimal::from(1000_u32),
            fixed_time(),
            "兼职",
        )?;
        let splits = service.set_transaction_splits(
            tx.id,
            &[input(salary, "800.00"), input(interest.id, "200.00")],
        )?;
        assert_eq!(splits.len(), 2);
        assert_eq!(sum_amounts(&splits), tx.amount);
        Ok(())
    }

    #[test]
    fn split_total_must_equal_transaction_amount() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        assert!(service
            .set_transaction_splits(tx.id, &[input(food, "60.00"), input(shopping, "30.00")])
            .is_err());
        // 校验失败不留下部分拆分。
        assert!(service.list_transaction_splits(tx.id)?.is_empty());
        Ok(())
    }

    #[test]
    fn expense_cannot_use_income_category() -> Result<()> {
        let (mut service, account, food, _, salary) = seeded()?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        assert!(service
            .set_transaction_splits(tx.id, &[input(salary, "100.00")])
            .is_err());
        Ok(())
    }

    #[test]
    fn zero_or_negative_split_amount_is_rejected() -> Result<()> {
        let (mut service, account, food, _, _) = seeded()?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        assert!(service
            .set_transaction_splits(tx.id, &[input(food, "0.00")])
            .is_err());
        Ok(())
    }

    #[test]
    fn setting_splits_atomically_replaces_old_ones() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        service.set_transaction_splits(tx.id, &[input(food, "60.00"), input(shopping, "40.00")])?;
        assert_eq!(service.list_transaction_splits(tx.id)?.len(), 2);
        // 整体替换为 1 行。
        let splits = service.set_transaction_splits(tx.id, &[input(shopping, "100.00")])?;
        assert_eq!(splits.len(), 1);
        assert_eq!(splits[0].category_id, shopping);
        Ok(())
    }

    #[test]
    fn clearing_splits_restores_parent_category_statistics() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        service.set_transaction_splits(tx.id, &[input(shopping, "100.00")])?;
        let summary = service.monthly_summary(2026, 8, "CNY")?;
        assert_eq!(summary.expenses_by_category.len(), 1);
        assert_eq!(summary.expenses_by_category[0].category_id, shopping);
        service.clear_transaction_splits(tx.id)?;
        let summary = service.monthly_summary(2026, 8, "CNY")?;
        assert_eq!(summary.expenses_by_category.len(), 1);
        assert_eq!(summary.expenses_by_category[0].category_id, food);
        Ok(())
    }

    #[test]
    fn splits_replace_parent_category_in_statistics_without_double_counting() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx = service.record_expense(
            account,
            food,
            Decimal::from(238_u32),
            fixed_time(),
            "沃尔玛",
        )?;
        service.set_transaction_splits(
            tx.id,
            &[
                input(food, "80.00"),
                input(shopping, "108.00"),
                input(food, "50.00"),
            ],
        )?;
        let summary = service.monthly_summary(2026, 8, "CNY")?;
        // 分类统计按拆分展开：餐饮 130、购物 108；父分类不重复计 238。
        assert_eq!(summary.total_expense, Decimal::from(238_u32));
        let by_id: Vec<(i64, Decimal)> = summary
            .expenses_by_category
            .iter()
            .map(|item| (item.category_id, item.amount))
            .collect();
        assert!(by_id.contains(&(food, Decimal::from(130_u32))));
        assert!(by_id.contains(&(shopping, Decimal::from(108_u32))));
        Ok(())
    }

    #[test]
    fn budgets_do_not_double_count_splits() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx = service.record_expense(
            account,
            food,
            Decimal::from(238_u32),
            fixed_time(),
            "沃尔玛",
        )?;
        service.set_transaction_splits(
            tx.id,
            &[
                input(food, "80.00"),
                input(shopping, "108.00"),
                input(food, "50.00"),
            ],
        )?;
        let summary = service.monthly_summary(2026, 8, "CNY")?;
        // Budget 面板使用 expenses_by_category：总支出 238，餐饮 130 / 购物 108，无双计。
        assert_eq!(summary.total_expense, Decimal::from(238_u32));
        let food_amount = summary
            .expenses_by_category
            .iter()
            .find(|item| item.category_id == food)
            .unwrap()
            .amount;
        assert_eq!(food_amount, Decimal::from(130_u32));
        Ok(())
    }

    #[test]
    fn deleting_transaction_removes_its_splits() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        service.set_transaction_splits(tx.id, &[input(food, "60.00"), input(shopping, "40.00")])?;
        service.void_transaction(tx.id)?;
        service.delete_transaction(tx.id)?;
        assert!(service.list_transaction_splits(tx.id)?.is_empty());
        Ok(())
    }

    #[test]
    fn void_and_restore_keep_splits() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        service.set_transaction_splits(tx.id, &[input(food, "60.00"), input(shopping, "40.00")])?;
        service.void_transaction(tx.id)?;
        assert_eq!(service.list_transaction_splits(tx.id)?.len(), 2);
        service.restore_transaction(tx.id)?;
        assert_eq!(service.list_transaction_splits(tx.id)?.len(), 2);
        Ok(())
    }

    #[test]
    fn changing_parent_amount_must_keep_splits_in_sync() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        service.set_transaction_splits(tx.id, &[input(food, "60.00"), input(shopping, "40.00")])?;
        // 改父金额为 120（splits 总和 100）→ 拒绝。
        assert!(service
            .update_transaction(
                tx.id,
                None,
                None,
                None,
                Some(Decimal::from(120_u32)),
                None,
                None,
            )
            .is_err());
        // 改为 100（与 splits 一致）→ 允许。
        service.update_transaction(
            tx.id,
            None,
            None,
            None,
            Some(Decimal::from(100_u32)),
            None,
            None,
        )?;
        Ok(())
    }

    #[test]
    fn non_expense_income_transactions_cannot_be_split() -> Result<()> {
        let (mut service, account, food, _, _) = seeded()?;
        let target = service.create_account(
            "储蓄",
            AccountType::Savings,
            "CNY",
            Decimal::from(10000_u32),
        )?;
        let transfer = service.record_transfer(
            account,
            target.id,
            Decimal::from(100_u32),
            Decimal::from(100_u32),
            fixed_time(),
            "转账",
        )?;
        assert!(service
            .set_transaction_splits(transfer.id, &[input(food, "100.00")])
            .is_err());
        Ok(())
    }
}
