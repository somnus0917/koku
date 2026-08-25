//! SQLite 行映射：Row 元组类型与 from_row 转换函数。
//!
//! 转换依赖（decimal/timestamp 等）仍定义在 `super`（service/mod.rs），
//! 通过 `pub(super) use rows::*` 在本模块与各业务子模块中共享。

use chrono::Utc;
use rust_decimal::Decimal;

use super::{calculate_simple_interest, decimal_from_db, parse_timestamp};
use crate::domain::{
    Account, AccountType, Category, CategoryKind, Loan, LoanType, Transaction, TransactionKind,
    User, UserRole,
};
use crate::error::Result;

pub(super) type AccountRow = (
    i64,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<i64>,
);
pub(super) type CategoryRow = (i64, String, String, Option<String>);
pub(super) type UserRow = (i64, String, String, String, i64, String, i64);
pub(super) type TransactionRow = (
    i64,
    String,
    i64,
    Option<i64>,
    Option<i64>,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
    String,
    String,
    Option<i64>,
    bool,
    String,
    Option<i64>,
    Option<String>,
    Option<String>,
    bool,
);

pub(super) fn account_from_row(row: AccountRow) -> Result<Account> {
    Ok(Account {
        id: row.0,
        name: row.1,
        account_type: AccountType::from_db(&row.2)?,
        currency: row.3,
        balance: decimal_from_db(&row.4)?,
        credit_limit: row.5.as_deref().map(decimal_from_db).transpose()?,
        statement_day: row.6.map(|day| day as u32),
        due_day: row.7.map(|day| day as u32),
    })
}

pub(super) fn user_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UserRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    ))
}

pub(super) fn user_from_row(row: UserRow) -> Result<User> {
    Ok(User {
        id: row.0,
        username: row.1,
        password_hash: row.2,
        role: UserRole::from_db(&row.3)?,
        enabled: row.4 != 0,
        created_at: parse_timestamp(&row.5)?,
        totp_enabled: row.6 != 0,
    })
}

pub(super) fn category_from_row(row: CategoryRow) -> Result<Category> {
    Ok(Category {
        id: row.0,
        name: row.1,
        kind: CategoryKind::from_db(&row.2)?,
        icon: row.3,
    })
}

pub(super) fn transaction_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TransactionRow> {
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
        row.get(14)?,
        row.get(15)?,
        row.get(16)?,
        row.get(17)?,
        row.get(18)?,
        row.get(19)?,
        row.get(20)?,
        row.get(21)?,
        row.get(22)?,
        row.get(23)?,
        row.get(24)?,
    ))
}

pub(super) fn transaction_from_row(row: TransactionRow) -> Result<Transaction> {
    Ok(Transaction {
        id: row.0,
        kind: TransactionKind::from_db(&row.1)?,
        account_id: row.2,
        to_account_id: row.3,
        category_id: row.4,
        amount: decimal_from_db(&row.5)?,
        currency: row.6,
        settled_amount: decimal_from_db(&row.7)?,
        target_amount: row.8.as_deref().map(decimal_from_db).transpose()?,
        target_currency: row.9,
        occurred_at: parse_timestamp(&row.10)?,
        note: row.11,
        voided_at: row.12.as_deref().map(parse_timestamp).transpose()?,
        loan_id: row.13,
        reimbursable_at: row.14.as_deref().map(parse_timestamp).transpose()?,
        reimbursed_at: row.15.as_deref().map(parse_timestamp).transpose()?,
        reimbursed_amount: decimal_from_db(&row.16)?,
        refunded_amount: decimal_from_db(&row.17)?,
        refund_expense_id: row.18,
        has_receipt: row.19,
        tags: split_tags(&row.20),
        payee_id: row.21,
        raw_description: row.22,
        payee_name: row.23,
        has_splits: row.24,
    })
}

/// 把 group_concat 得到的逗号分隔标签串拆成去空白的标签名列表。
pub(super) fn split_tags(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(super) type LoanRow = (
    i64,
    String,
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

pub(super) fn loan_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LoanRow> {
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
    ))
}

pub(super) fn loan_from_row(row: LoanRow) -> Result<Loan> {
    let principal = decimal_from_db(&row.4)?;
    let opened_at = parse_timestamp(&row.7)?;
    let closed_at = row.9.as_deref().map(parse_timestamp).transpose()?;
    let interest_rate = row.11.as_deref().map(decimal_from_db).transpose()?;
    let accrued_interest = interest_rate
        .map(|rate| {
            calculate_simple_interest(
                principal,
                rate,
                opened_at,
                closed_at.unwrap_or_else(Utc::now),
            )
        })
        .unwrap_or(Decimal::ZERO);
    Ok(Loan {
        id: row.0,
        loan_type: LoanType::from_db(&row.1)?,
        counterparty: row.2,
        currency: row.3,
        principal,
        outstanding: decimal_from_db(&row.5)?,
        account_id: row.6,
        opened_at,
        note: row.8,
        closed_at,
        due_at: row.10.as_deref().map(parse_timestamp).transpose()?,
        interest_rate,
        accrued_interest,
    })
}
