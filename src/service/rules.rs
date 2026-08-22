//! 可解释的交易规则：主要用于导入后的商户清洗和分类。

use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use rust_decimal::Decimal;

use super::*;
use crate::domain::{TransactionKind, TransactionRule, TransactionRulePreview};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

#[derive(Debug, Clone)]
pub struct TransactionRuleInput {
    pub name: String,
    pub enabled: bool,
    pub priority: i64,
    pub description_contains: Option<String>,
    pub account_id: Option<i64>,
    pub kind: Option<TransactionKind>,
    pub min_amount: Option<Decimal>,
    pub max_amount: Option<Decimal>,
    pub category_id: Option<i64>,
    pub payee_name: Option<String>,
    pub tag_names: Vec<String>,
}

impl BookkeepingService {
    pub fn transaction_rules(&self) -> Result<Vec<TransactionRule>> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, enabled, priority, description_contains, account_id, kind, min_amount, max_amount, category_id, payee_name, tag_names, created_at, updated_at FROM transaction_rules ORDER BY priority, id",
        )?;
        let rules = statement
            .query_map([], rule_row)?
            .map(|row| rule_from_row(row?))
            .collect();
        rules
    }

    pub fn create_transaction_rule(
        &mut self,
        input: TransactionRuleInput,
    ) -> Result<TransactionRule> {
        validate_rule_input(self, &input)?;
        let now = timestamp(Utc::now());
        let tag_names = serde_json::to_string(&input.tag_names)
            .map_err(|error| KokuError::InvalidInput(format!("invalid rule tags: {error}")))?;
        self.conn.execute(
            "INSERT INTO transaction_rules(name, enabled, priority, description_contains, account_id, kind, min_amount, max_amount, category_id, payee_name, tag_names, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
            params![input.name.trim(), input.enabled, input.priority, optional_trimmed(input.description_contains), input.account_id, input.kind.map(TransactionKind::as_str), input.min_amount.map(decimal_to_db), input.max_amount.map(decimal_to_db), input.category_id, optional_trimmed(input.payee_name), tag_names, now],
        )?;
        self.transaction_rule(self.conn.last_insert_rowid())
    }

    pub fn update_transaction_rule(
        &mut self,
        id: i64,
        input: TransactionRuleInput,
    ) -> Result<TransactionRule> {
        validate_rule_input(self, &input)?;
        let tag_names = serde_json::to_string(&input.tag_names)
            .map_err(|error| KokuError::InvalidInput(format!("invalid rule tags: {error}")))?;
        let changed = self.conn.execute(
            "UPDATE transaction_rules SET name=?1, enabled=?2, priority=?3, description_contains=?4, account_id=?5, kind=?6, min_amount=?7, max_amount=?8, category_id=?9, payee_name=?10, tag_names=?11, updated_at=?12 WHERE id=?13",
            params![input.name.trim(), input.enabled, input.priority, optional_trimmed(input.description_contains), input.account_id, input.kind.map(TransactionKind::as_str), input.min_amount.map(decimal_to_db), input.max_amount.map(decimal_to_db), input.category_id, optional_trimmed(input.payee_name), tag_names, timestamp(Utc::now()), id],
        )?;
        if changed != 1 {
            return Err(KokuError::NotFound {
                entity: "transaction rule",
                id,
            });
        }
        self.transaction_rule(id)
    }

    pub fn delete_transaction_rule(&mut self, id: i64) -> Result<()> {
        if self
            .conn
            .execute("DELETE FROM transaction_rules WHERE id=?1", [id])?
            != 1
        {
            return Err(KokuError::NotFound {
                entity: "transaction rule",
                id,
            });
        }
        Ok(())
    }

    pub fn apply_rules_to_transaction(&mut self, transaction_id: i64) -> Result<usize> {
        let mut transaction = self.transaction(transaction_id)?;
        if transaction.voided_at.is_some()
            || !matches!(
                transaction.kind,
                TransactionKind::Expense | TransactionKind::Income
            )
        {
            return Ok(0);
        }
        let mut applied = 0;
        for rule in self
            .transaction_rules()?
            .into_iter()
            .filter(|rule| rule.enabled)
        {
            if !rule_matches(&rule, &transaction) {
                continue;
            }
            if rule.category_id.is_none() && rule.payee_name.is_none() && rule.tag_names.is_empty()
            {
                continue;
            }
            transaction = self.update_transaction_edit(
                transaction.id,
                None,
                None,
                rule.category_id,
                None,
                None,
                None,
                None,
                rule.payee_name.as_deref(),
                if rule.tag_names.is_empty() {
                    None
                } else {
                    Some(&rule.tag_names)
                },
                false,
            )?;
            applied += 1;
        }
        Ok(applied)
    }

    /// 返回真正会改变数据的历史匹配项；前端必须先展示这些项并要求用户确认。
    pub fn preview_transaction_rule(&self, rule_id: i64) -> Result<Vec<TransactionRulePreview>> {
        let rule = self.transaction_rule(rule_id)?;
        if !rule.enabled {
            return Ok(Vec::new());
        }
        let ids: Vec<i64> = self
            .conn
            .prepare("SELECT id FROM transactions ORDER BY id")?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut previews = Vec::new();
        for id in ids {
            let transaction = self.transaction(id)?;
            if rule_matches(&rule, &transaction) && rule_changes_transaction(&rule, &transaction) {
                previews.push(TransactionRulePreview {
                    transaction_id: transaction.id,
                    occurred_at: transaction.occurred_at,
                    note: transaction.note.clone(),
                    amount: transaction.amount,
                    currency: transaction.currency.clone(),
                    current_category_id: transaction.category_id,
                    suggested_category_id: rule.category_id.or(transaction.category_id),
                    current_payee_name: transaction.payee_name.clone(),
                    suggested_payee_name: rule
                        .payee_name
                        .clone()
                        .or(transaction.payee_name.clone()),
                    current_tags: transaction.tags.clone(),
                    suggested_tags: if rule.tag_names.is_empty() {
                        transaction.tags.clone()
                    } else {
                        rule.tag_names.clone()
                    },
                });
            }
        }
        Ok(previews)
    }

    /// 应用预览中由用户确认的指定流水。过期或已不匹配的候选会被安全跳过。
    pub fn apply_transaction_rule_preview(
        &mut self,
        rule_id: i64,
        transaction_ids: &[i64],
    ) -> Result<usize> {
        let rule = self.transaction_rule(rule_id)?;
        if !rule.enabled {
            return Ok(0);
        }
        let mut changed = 0;
        for id in transaction_ids {
            let transaction = self.transaction(*id)?;
            if rule_matches(&rule, &transaction) && rule_changes_transaction(&rule, &transaction) {
                self.apply_rule_to_transaction(&rule, transaction.id)?;
                changed += 1;
            }
        }
        Ok(changed)
    }

    fn apply_rule_to_transaction(
        &mut self,
        rule: &TransactionRule,
        transaction_id: i64,
    ) -> Result<()> {
        self.update_transaction_edit(
            transaction_id,
            None,
            None,
            rule.category_id,
            None,
            None,
            None,
            None,
            rule.payee_name.as_deref(),
            if rule.tag_names.is_empty() {
                None
            } else {
                Some(&rule.tag_names)
            },
            false,
        )?;
        Ok(())
    }

    fn transaction_rule(&self, id: i64) -> Result<TransactionRule> {
        self.conn.query_row(
            "SELECT id, name, enabled, priority, description_contains, account_id, kind, min_amount, max_amount, category_id, payee_name, tag_names, created_at, updated_at FROM transaction_rules WHERE id=?1", [id], rule_row,
        ).optional()?.map(rule_from_row).transpose()?.ok_or(KokuError::NotFound { entity: "transaction rule", id })
    }
}

type RuleRow = (
    i64,
    String,
    bool,
    i64,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    String,
    String,
    String,
);
fn rule_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RuleRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
    ))
}
fn rule_from_row(row: RuleRow) -> Result<TransactionRule> {
    Ok(TransactionRule {
        id: row.0,
        name: row.1,
        enabled: row.2,
        priority: row.3,
        description_contains: row.4,
        account_id: row.5,
        kind: row.6.as_deref().map(TransactionKind::from_db).transpose()?,
        min_amount: row.7.as_deref().map(decimal_from_db).transpose()?,
        max_amount: row.8.as_deref().map(decimal_from_db).transpose()?,
        category_id: row.9,
        payee_name: row.10,
        tag_names: serde_json::from_str(&row.11)
            .map_err(|e| KokuError::InvalidInput(format!("invalid rule tags: {e}")))?,
        created_at: parse_timestamp(&row.12)?,
        updated_at: parse_timestamp(&row.13)?,
    })
}
fn optional_trimmed(value: Option<String>) -> Option<String> {
    value.and_then(|v| {
        let v = v.trim().to_owned();
        (!v.is_empty()).then_some(v)
    })
}
fn validate_rule_input(service: &BookkeepingService, input: &TransactionRuleInput) -> Result<()> {
    if input.name.trim().is_empty() {
        return Err(KokuError::InvalidInput(
            "rule name cannot be empty".to_owned(),
        ));
    }
    if input.name.chars().count() > 80 {
        return Err(KokuError::InvalidInput(
            "rule name must be 80 characters or fewer".to_owned(),
        ));
    }
    if let Some(id) = input.account_id {
        service.account(id)?;
    }
    if let Some(id) = input.category_id {
        let c = service.category(id)?;
        if let Some(kind) = input.kind {
            let expected = if kind == TransactionKind::Expense {
                crate::domain::CategoryKind::Expense
            } else {
                crate::domain::CategoryKind::Income
            };
            if c.kind != expected {
                return Err(KokuError::CategoryKindMismatch {
                    expected: expected.as_str(),
                    actual: c.kind.as_str(),
                });
            }
        }
    }
    if input.min_amount.is_some_and(|x| x < Decimal::ZERO)
        || input.max_amount.is_some_and(|x| x < Decimal::ZERO)
        || input
            .min_amount
            .zip(input.max_amount)
            .is_some_and(|(a, b)| a > b)
    {
        return Err(KokuError::InvalidInput(
            "invalid rule amount range".to_owned(),
        ));
    }
    for tag in &input.tag_names {
        validate_tag_name(tag)?;
    }
    Ok(())
}
fn rule_matches(rule: &TransactionRule, tx: &crate::domain::Transaction) -> bool {
    if rule.account_id.is_some_and(|id| id != tx.account_id)
        || rule.kind.is_some_and(|kind| kind != tx.kind)
        || rule.min_amount.is_some_and(|x| tx.amount < x)
        || rule.max_amount.is_some_and(|x| tx.amount > x)
    {
        return false;
    }
    if let Some(needle) = &rule.description_contains {
        let value = format!(
            "{} {} {}",
            tx.note,
            tx.raw_description.clone().unwrap_or_default(),
            tx.payee_name.clone().unwrap_or_default()
        )
        .to_lowercase();
        if !value.contains(&needle.to_lowercase()) {
            return false;
        }
    }
    true
}

fn rule_changes_transaction(rule: &TransactionRule, tx: &crate::domain::Transaction) -> bool {
    if tx.voided_at.is_some()
        || !matches!(tx.kind, TransactionKind::Expense | TransactionKind::Income)
    {
        return false;
    }
    rule.category_id
        .is_some_and(|id| tx.category_id != Some(id))
        || rule
            .payee_name
            .as_deref()
            .is_some_and(|name| tx.payee_name.as_deref() != Some(name))
        || (!rule.tag_names.is_empty() && tx.tags != rule.tag_names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AccountType, CategoryKind};

    #[test]
    fn rules_apply_category_payee_and_tags_to_matching_transactions() -> Result<()> {
        let mut service = BookkeepingService::in_memory()?;
        let account = service.create_account("现金", AccountType::Cash, "CNY", Decimal::ZERO)?;
        let food = service.create_category("餐饮", CategoryKind::Expense)?;
        let transport = service.create_category("交通", CategoryKind::Expense)?;
        let transaction = service.record_expense(
            account.id,
            food.id,
            Decimal::from(20_u32),
            Utc::now(),
            "滴滴出行",
        )?;
        let rule = service.create_transaction_rule(TransactionRuleInput {
            name: "滴滴".into(),
            enabled: true,
            priority: 0,
            description_contains: Some("滴滴".into()),
            account_id: Some(account.id),
            kind: Some(TransactionKind::Expense),
            min_amount: None,
            max_amount: None,
            category_id: Some(transport.id),
            payee_name: Some("滴滴出行".into()),
            tag_names: vec!["通勤".into()],
        })?;
        let previews = service.preview_transaction_rule(rule.id)?;
        assert_eq!(previews.len(), 1);
        assert_eq!(
            service.apply_transaction_rule_preview(rule.id, &[transaction.id])?,
            1
        );
        let updated = service.transaction(transaction.id)?;
        assert_eq!(updated.category_id, Some(transport.id));
        assert_eq!(updated.payee_name.as_deref(), Some("滴滴出行"));
        assert_eq!(updated.tags, vec!["通勤"]);
        Ok(())
    }
}
