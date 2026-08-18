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

    /// 原子替换交易的拆分。
    ///
    /// 空数组（`&[]`）等价于清除拆分，恢复父交易分类统计，不做金额校验；
    /// 非空数组校验顺序：交易存在 → 类型支持拆分 → 各拆分分类存在且类型匹配 →
    /// 金额 > 0 → 总和等于父交易金额 → 整体替换；任一步失败全部回滚。
    /// 拆分一旦存在，该交易此前的单分类学习样本在同一事务内撤销
    /// （有拆分的交易不参与 Payee → Category 学习，见 `confirm_transaction_learning`）。
    pub fn set_transaction_splits(
        &mut self,
        transaction_id: i64,
        splits: &[SplitInput],
    ) -> Result<Vec<TransactionSplit>> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let transaction = Self::transaction_in_tx(&tx, transaction_id)?;
        // 空数组 = 清除；非空数组才校验总和 == 父交易金额。
        if !splits.is_empty() {
            Self::validate_splits_in_tx(&tx, transaction.kind, splits, transaction.amount)?;
        }
        Self::replace_transaction_splits_in_tx(&tx, transaction_id, splits)?;
        if !splits.is_empty() {
            Self::revoke_transaction_learning_in_tx(&tx, transaction_id)?;
        }
        tx.commit()?;
        self.list_transaction_splits(transaction_id)
    }

    /// 事务内：交易是否存在 ≥1 条拆分。
    pub(super) fn transaction_has_splits_in_tx(
        tx: &SqlTransaction<'_>,
        transaction_id: i64,
    ) -> Result<bool> {
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM transaction_splits WHERE transaction_id = ?1",
            [transaction_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// 事务内：校验拆分（仅 expense/income、分类存在且类型匹配、金额 > 0、
    /// 总和等于期望值）。任一步失败即返回错误，调用方负责回滚。
    pub(super) fn validate_splits_in_tx(
        tx: &SqlTransaction<'_>,
        kind: TransactionKind,
        splits: &[SplitInput],
        expected_total: Decimal,
    ) -> Result<()> {
        let expected_kind = match kind {
            TransactionKind::Expense => CategoryKind::Expense,
            TransactionKind::Income => CategoryKind::Income,
            _ => {
                return Err(KokuError::InvalidInput(
                    "only expense and income transactions can be split".to_owned(),
                ))
            }
        };
        let mut total = Decimal::ZERO;
        for split in splits {
            if split.amount <= Decimal::ZERO {
                return Err(KokuError::InvalidInput(
                    "split amount must be greater than zero".to_owned(),
                ));
            }
            let category = Self::category_in_tx(tx, split.category_id)?;
            if category.kind != expected_kind {
                return Err(KokuError::CategoryKindMismatch {
                    expected: expected_kind.as_str(),
                    actual: category.kind.as_str(),
                });
            }
            total += split.amount;
        }
        if total != expected_total {
            return Err(KokuError::InvalidInput(format!(
                "split amounts must sum to the transaction amount ({expected_total})"
            )));
        }
        Ok(())
    }

    /// 事务内：整体替换交易拆分（DELETE + INSERT，同一事务）。
    pub(super) fn replace_transaction_splits_in_tx(
        tx: &SqlTransaction<'_>,
        transaction_id: i64,
        splits: &[SplitInput],
    ) -> Result<()> {
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
        Ok(())
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

    // ------------------------------------------------------------------
    // 原子更新：父交易字段与拆分同事务提交（P0 收尾）
    // ------------------------------------------------------------------

    #[test]
    fn atomic_update_changes_amount_and_splits_together() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        service.set_transaction_splits(tx.id, &[input(food, "60.00"), input(shopping, "40.00")])?;
        // 一次更新：金额 100 → 120，拆分 60+40 → 70+50（总和 == 新金额）。
        let updated = service.update_transaction_edit(
            tx.id,
            None,
            None,
            None,
            Some(Decimal::from(120_u32)),
            None,
            None,
            Some(&[input(food, "70.00"), input(shopping, "50.00")]),
            None,
            None,
            false,
        )?;
        assert_eq!(updated.amount, Decimal::from(120_u32));
        let splits = service.list_transaction_splits(tx.id)?;
        assert_eq!(sum_amounts(&splits), updated.amount);
        let by_category: Vec<(i64, Decimal)> =
            splits.iter().map(|s| (s.category_id, s.amount)).collect();
        assert!(by_category.contains(&(food, Decimal::from(70_u32))));
        assert!(by_category.contains(&(shopping, Decimal::from(50_u32))));
        Ok(())
    }

    #[test]
    fn atomic_update_with_mismatched_splits_rolls_back_everything() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        service.set_transaction_splits(tx.id, &[input(food, "60.00"), input(shopping, "40.00")])?;
        let balance_before = service.account(account)?.balance;
        // 金额 120 + 拆分 70+40（总和 110 ≠ 120）→ 校验失败，父交易与拆分全部回滚。
        assert!(service
            .update_transaction_edit(
                tx.id,
                None,
                None,
                None,
                Some(Decimal::from(120_u32)),
                None,
                None,
                Some(&[input(food, "70.00"), input(shopping, "40.00")]),
                None,
                None,
                false,
            )
            .is_err());
        let transaction = service.transaction(tx.id)?;
        assert_eq!(transaction.amount, Decimal::from(100_u32));
        let splits = service.list_transaction_splits(tx.id)?;
        let by_category: Vec<(i64, Decimal)> =
            splits.iter().map(|s| (s.category_id, s.amount)).collect();
        assert_eq!(by_category.len(), 2);
        assert!(by_category.contains(&(food, Decimal::from(60_u32))));
        assert!(by_category.contains(&(shopping, Decimal::from(40_u32))));
        assert_eq!(service.account(account)?.balance, balance_before);
        Ok(())
    }

    #[test]
    fn amount_only_change_without_splits_is_rejected_when_splits_exist() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        service.set_transaction_splits(tx.id, &[input(food, "60.00"), input(shopping, "40.00")])?;
        // 不带 splits 的金额修改（120 ≠ 旧拆分总和 100）→ 拒绝且无任何修改。
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
        assert_eq!(service.transaction(tx.id)?.amount, Decimal::from(100_u32));
        assert_eq!(service.list_transaction_splits(tx.id)?.len(), 2);
        Ok(())
    }

    #[test]
    fn split_only_change_updates_splits_keeping_parent_amount() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        service.set_transaction_splits(tx.id, &[input(food, "60.00"), input(shopping, "40.00")])?;
        let updated = service.update_transaction_edit(
            tx.id,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&[input(shopping, "30.00"), input(food, "70.00")]),
            None,
            None,
            false,
        )?;
        // 父金额不变，拆分整体替换。
        assert_eq!(updated.amount, Decimal::from(100_u32));
        let splits = service.list_transaction_splits(tx.id)?;
        assert_eq!(sum_amounts(&splits), updated.amount);
        let by_category: Vec<(i64, Decimal)> =
            splits.iter().map(|s| (s.category_id, s.amount)).collect();
        assert!(by_category.contains(&(shopping, Decimal::from(30_u32))));
        assert!(by_category.contains(&(food, Decimal::from(70_u32))));
        Ok(())
    }

    #[test]
    fn combined_field_update_with_invalid_split_keeps_all_old_values() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx = service.record_expense(
            account,
            food,
            Decimal::from(100_u32),
            fixed_time(),
            "旧备注",
        )?;
        service.set_transaction_splits(tx.id, &[input(food, "60.00"), input(shopping, "40.00")])?;
        // 备注 + 分类 + 金额一起改，但拆分总和与金额不符 → 全部回滚，保持旧值。
        assert!(service
            .update_transaction_edit(
                tx.id,
                Some("新备注".to_owned()),
                None,
                Some(shopping),
                Some(Decimal::from(120_u32)),
                None,
                None,
                Some(&[input(food, "70.00"), input(shopping, "30.00")]),
                None,
                None,
                false,
            )
            .is_err());
        let transaction = service.transaction(tx.id)?;
        assert_eq!(transaction.note, "旧备注");
        assert_eq!(transaction.category_id, Some(food));
        assert_eq!(transaction.amount, Decimal::from(100_u32));
        assert_eq!(service.list_transaction_splits(tx.id)?.len(), 2);
        Ok(())
    }

    #[test]
    fn clearing_splits_via_atomic_update_removes_them() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        service.set_transaction_splits(tx.id, &[input(food, "60.00"), input(shopping, "40.00")])?;
        let updated = service.update_transaction_edit(
            tx.id,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&[]),
            None,
            None,
            false,
        )?;
        assert_eq!(updated.amount, Decimal::from(100_u32));
        assert!(service.list_transaction_splits(tx.id)?.is_empty());
        Ok(())
    }

    #[test]
    fn set_transaction_splits_with_empty_list_clears_splits() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        service.set_transaction_splits(tx.id, &[input(shopping, "100.00")])?;
        // 空数组 = 清除（服务层与 PUT /splits 共用此路径）。
        service.set_transaction_splits(tx.id, &[])?;
        assert!(service.list_transaction_splits(tx.id)?.is_empty());
        // 统计回到父分类。
        let summary = service.monthly_summary(2026, 8, "CNY")?;
        assert_eq!(summary.expenses_by_category.len(), 1);
        assert_eq!(summary.expenses_by_category[0].category_id, food);
        Ok(())
    }

    // ------------------------------------------------------------------
    // 学习边界：有拆分的交易不参与 Payee → Category 学习（P1）
    // ------------------------------------------------------------------

    fn learning_row_count(service: &BookkeepingService, transaction_id: i64) -> Result<i64> {
        Ok(service.conn.query_row(
            "SELECT COUNT(*) FROM transaction_learning WHERE transaction_id = ?1",
            [transaction_id],
            |row| row.get(0),
        )?)
    }

    #[test]
    fn setting_splits_revokes_prior_learning_in_same_transaction() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let payee = service.get_or_create_payee("星巴克")?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        service.conn.execute(
            "UPDATE transactions SET payee_id = ?1 WHERE id = ?2",
            params![payee.id, tx.id],
        )?;
        service.confirm_transaction_learning(tx.id)?;
        assert_eq!(learning_row_count(&service, tx.id)?, 1);
        // 设置拆分（无 → 有）：同一事务内撤销学习样本。
        service.set_transaction_splits(tx.id, &[input(food, "60.00"), input(shopping, "40.00")])?;
        assert_eq!(learning_row_count(&service, tx.id)?, 0);
        let stats: i64 = service.conn.query_row(
            "SELECT COUNT(*) FROM payee_category_stats WHERE payee_id = ?1",
            [payee.id],
            |row| row.get(0),
        )?;
        assert_eq!(stats, 0);
        Ok(())
    }

    #[test]
    fn atomic_update_adding_splits_revokes_prior_learning() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let payee = service.get_or_create_payee("星巴克")?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        service.conn.execute(
            "UPDATE transactions SET payee_id = ?1 WHERE id = ?2",
            params![payee.id, tx.id],
        )?;
        service.confirm_transaction_learning(tx.id)?;
        assert_eq!(learning_row_count(&service, tx.id)?, 1);
        // 原子路径（无 → 有拆分）：同一事务内撤销学习。
        service.update_transaction_edit(
            tx.id,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&[input(food, "60.00"), input(shopping, "40.00")]),
            None,
            None,
            false,
        )?;
        assert_eq!(learning_row_count(&service, tx.id)?, 0);
        Ok(())
    }

    #[test]
    fn clearing_splits_does_not_auto_relearn() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let payee = service.get_or_create_payee("星巴克")?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        service.conn.execute(
            "UPDATE transactions SET payee_id = ?1 WHERE id = ?2",
            params![payee.id, tx.id],
        )?;
        service.set_transaction_splits(tx.id, &[input(food, "60.00"), input(shopping, "40.00")])?;
        service.clear_transaction_splits(tx.id)?;
        // 清除拆分不自动重学父分类；只有后续显式 PATCH Payee/Category 才触发确认。
        assert_eq!(learning_row_count(&service, tx.id)?, 0);
        Ok(())
    }

    #[test]
    fn confirm_on_split_transaction_revokes_learning_and_adds_nothing() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let payee = service.get_or_create_payee("星巴克")?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        service.conn.execute(
            "UPDATE transactions SET payee_id = ?1 WHERE id = ?2",
            params![payee.id, tx.id],
        )?;
        service.confirm_transaction_learning(tx.id)?;
        service.set_transaction_splits(tx.id, &[input(food, "60.00"), input(shopping, "40.00")])?;
        // 带拆分的交易再次 confirm：撤销旧贡献且不新增样本。
        service.confirm_transaction_learning(tx.id)?;
        assert_eq!(learning_row_count(&service, tx.id)?, 0);
        let stats: i64 = service.conn.query_row(
            "SELECT COUNT(*) FROM payee_category_stats WHERE payee_id = ?1",
            [payee.id],
            |row| row.get(0),
        )?;
        assert_eq!(stats, 0);
        Ok(())
    }

    #[test]
    fn transaction_payload_exposes_has_splits_flag() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        assert!(!service.transaction(tx.id)?.has_splits);
        service.set_transaction_splits(tx.id, &[input(food, "60.00"), input(shopping, "40.00")])?;
        assert!(service.transaction(tx.id)?.has_splits);
        // 列表查询同样暴露标记（供前端列表打标，无 N+1）。
        let listed = service.transactions(10, 0)?;
        assert!(listed
            .iter()
            .any(|item| item.id == tx.id && item.has_splits));
        service.clear_transaction_splits(tx.id)?;
        assert!(!service.transaction(tx.id)?.has_splits);
        Ok(())
    }

    #[test]
    fn request_with_payee_and_splits_never_creates_a_sample() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx =
            service.record_expense(account, food, Decimal::from(100_u32), fixed_time(), "测试")?;
        // 模拟一次 PATCH：改 Payee + 带拆分（API 流程：原子更新 → 设 Payee → confirm）。
        service.update_transaction_edit(
            tx.id,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&[input(food, "60.00"), input(shopping, "40.00")]),
            None,
            None,
            false,
        )?;
        service.set_transaction_payee(tx.id, Some("瑞幸"))?;
        // 学习判断依据提交后最终状态：有拆分 → 不产生样本。
        service.confirm_transaction_learning(tx.id)?;
        assert_eq!(learning_row_count(&service, tx.id)?, 0);
        let stats: i64 =
            service
                .conn
                .query_row("SELECT COUNT(*) FROM payee_category_stats", [], |row| {
                    row.get(0)
                })?;
        assert_eq!(stats, 0);
        Ok(())
    }

    // ------------------------------------------------------------------
    // 整次编辑原子性：父字段 + Payee + 标签 + 拆分 + 学习一次事务（收尾）
    // ------------------------------------------------------------------

    #[test]
    fn atomic_edit_with_invalid_tag_rolls_back_everything() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx = service.record_expense(
            account,
            food,
            Decimal::from(100_u32),
            fixed_time(),
            "旧备注",
        )?;
        service.set_transaction_splits(tx.id, &[input(food, "60.00"), input(shopping, "40.00")])?;
        let balance_before = service.account(account)?.balance;
        // 金额 120 + 拆分 70+50（本身合法）+ 非法标签（含逗号）→ 标签校验失败，
        // 前面已写入的金额/备注/拆分/余额全部回滚。
        assert!(service
            .update_transaction_edit(
                tx.id,
                Some("新备注".to_owned()),
                None,
                None,
                Some(Decimal::from(120_u32)),
                None,
                None,
                Some(&[input(food, "70.00"), input(shopping, "50.00")]),
                None,
                Some(&["好,标签".to_owned()]),
                false,
            )
            .is_err());
        let transaction = service.transaction(tx.id)?;
        assert_eq!(transaction.note, "旧备注");
        assert_eq!(transaction.amount, Decimal::from(100_u32));
        assert_eq!(transaction.category_id, Some(food));
        let splits = service.list_transaction_splits(tx.id)?;
        assert_eq!(splits.len(), 2);
        assert_eq!(sum_amounts(&splits), Decimal::from(100_u32));
        assert_eq!(service.account(account)?.balance, balance_before);
        assert!(service.all_tags()?.is_empty());
        Ok(())
    }

    #[test]
    fn payee_failure_rolls_back_other_fields() -> Result<()> {
        let (mut service, account, food, _, _) = seeded()?;
        let tx = service.record_expense(
            account,
            food,
            Decimal::from(100_u32),
            fixed_time(),
            "旧备注",
        )?;
        // 给交易一份原始描述，使 Payee 变更会触发 alias 学习。
        service.set_import_metadata(tx.id, "星巴克 - 早餐", None, None)?;
        // 移除 merchant_aliases 表，让 Payee 步骤的 alias 学习失败。
        service.conn.execute("DROP TABLE merchant_aliases", [])?;
        // 改 note + amount + payee：Payee 步骤失败 → 前面的备注/金额全部不落库。
        assert!(service
            .update_transaction_edit(
                tx.id,
                Some("新备注".to_owned()),
                None,
                None,
                Some(Decimal::from(120_u32)),
                None,
                None,
                None,
                Some("星巴克"),
                None,
                false,
            )
            .is_err());
        let transaction = service.transaction(tx.id)?;
        assert_eq!(transaction.note, "旧备注");
        assert_eq!(transaction.amount, Decimal::from(100_u32));
        assert_eq!(transaction.payee_id, None);
        assert_eq!(
            transaction.raw_description.as_deref(),
            Some("星巴克 - 早餐")
        );
        Ok(())
    }

    #[test]
    fn successful_edit_commits_everything_together() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx = service.record_expense(
            account,
            food,
            Decimal::from(100_u32),
            fixed_time(),
            "旧备注",
        )?;
        let updated = service.update_transaction_edit(
            tx.id,
            Some("新备注".to_owned()),
            None,
            Some(shopping),
            Some(Decimal::from(120_u32)),
            None,
            None,
            None,
            Some("星巴克"),
            Some(&["通勤".to_owned(), "通勤".to_owned()]),
            true,
        )?;
        assert_eq!(updated.note, "新备注");
        assert_eq!(updated.amount, Decimal::from(120_u32));
        assert_eq!(updated.category_id, Some(shopping));
        assert_eq!(updated.payee_name.as_deref(), Some("星巴克"));
        assert_eq!(updated.tags, vec!["通勤"]);
        // 学习确认基于提交后最终状态：(星巴克, 购物) 一份样本。
        let payee = service.get_or_create_payee("星巴克")?;
        let stats: u64 = service.conn.query_row(
            "SELECT count FROM payee_category_stats WHERE payee_id = ?1 AND category_id = ?2",
            params![payee.id, shopping],
            |row| row.get(0),
        )?;
        assert_eq!(stats, 1);
        assert_eq!(learning_row_count(&service, tx.id)?, 1);
        Ok(())
    }

    #[test]
    fn edit_with_splits_payee_and_tags_commits_together_without_learning() -> Result<()> {
        let (mut service, account, food, shopping, _) = seeded()?;
        let tx = service.record_expense(
            account,
            food,
            Decimal::from(100_u32),
            fixed_time(),
            "旧备注",
        )?;
        let updated = service.update_transaction_edit(
            tx.id,
            Some("新备注".to_owned()),
            None,
            None,
            None,
            None,
            None,
            Some(&[input(food, "60.00"), input(shopping, "40.00")]),
            Some("星巴克"),
            Some(&["超市".to_owned()]),
            true,
        )?;
        assert_eq!(updated.note, "新备注");
        assert_eq!(updated.payee_name.as_deref(), Some("星巴克"));
        assert_eq!(updated.tags, vec!["超市"]);
        assert_eq!(service.list_transaction_splits(tx.id)?.len(), 2);
        // 有拆分：仍不产生单一 Payee → Category 学习样本。
        assert_eq!(learning_row_count(&service, tx.id)?, 0);
        let stats: i64 =
            service
                .conn
                .query_row("SELECT COUNT(*) FROM payee_category_stats", [], |row| {
                    row.get(0)
                })?;
        assert_eq!(stats, 0);
        Ok(())
    }
}
