use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Datelike, NaiveDate, SecondsFormat, Utc};
use rusqlite::{
    params, Connection, OptionalExtension, Transaction as SqlTransaction, TransactionBehavior,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tower_http::cors::{Any, CorsLayer};

type Result<T> = std::result::Result<T, KokuError>;

#[derive(Debug, Error)]
pub enum KokuError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("invalid decimal stored in database: {0}")]
    InvalidDecimal(#[from] rust_decimal::Error),
    #[error("{entity} with id {id} was not found")]
    NotFound { entity: &'static str, id: i64 },
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("category kind must be {expected}, but was {actual}")]
    CategoryKindMismatch {
        expected: &'static str,
        actual: &'static str,
    },
    #[error("a transaction that has already been voided cannot be voided again")]
    AlreadyVoided,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountType {
    Asset,
    Liability,
}

impl AccountType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Asset => "asset",
            Self::Liability => "liability",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "asset" => Ok(Self::Asset),
            "liability" => Ok(Self::Liability),
            other => Err(KokuError::InvalidInput(format!(
                "unknown account type in database: {other}"
            ))),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Asset => "资产",
            Self::Liability => "负债",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CategoryKind {
    Expense,
    Income,
}

impl CategoryKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Expense => "expense",
            Self::Income => "income",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "expense" => Ok(Self::Expense),
            "income" => Ok(Self::Income),
            other => Err(KokuError::InvalidInput(format!(
                "unknown category kind in database: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionKind {
    Expense,
    Income,
    Transfer,
}

impl TransactionKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Expense => "expense",
            Self::Income => "income",
            Self::Transfer => "transfer",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "expense" => Ok(Self::Expense),
            "income" => Ok(Self::Income),
            "transfer" => Ok(Self::Transfer),
            other => Err(KokuError::InvalidInput(format!(
                "unknown transaction kind in database: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub account_type: AccountType,
    pub currency: String,
    pub balance: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub kind: CategoryKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    pub id: i64,
    pub kind: TransactionKind,
    pub account_id: i64,
    pub to_account_id: Option<i64>,
    pub category_id: Option<i64>,
    pub amount: Decimal,
    pub target_amount: Option<Decimal>,
    pub occurred_at: DateTime<Utc>,
    pub note: String,
    pub voided_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryExpense {
    pub category_id: i64,
    pub category_name: String,
    pub amount: Decimal,
    pub percentage: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonthlySummary {
    pub year: i32,
    pub month: u32,
    pub currency: String,
    pub total_income: Decimal,
    pub total_expense: Decimal,
    pub net: Decimal,
    pub expenses_by_category: Vec<CategoryExpense>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CashFlowItem {
    pub category_id: i64,
    pub category_name: String,
    pub amount: Decimal,
    pub percentage: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CashFlowSummary {
    pub year: i32,
    pub month: u32,
    pub currency: String,
    pub total_income: Decimal,
    pub total_expense: Decimal,
    pub retained: Decimal,
    pub flow_total: Decimal,
    pub income_sources: Vec<CashFlowItem>,
    pub expense_destinations: Vec<CashFlowItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BalanceSummary {
    pub currency: String,
    pub total_assets: Decimal,
    pub total_liabilities: Decimal,
    pub net_worth: Decimal,
}

#[derive(Debug, Serialize)]
struct ApiResponse<T> {
    data: T,
}

impl<T> ApiResponse<T> {
    fn new(data: T) -> Self {
        Self { data }
    }
}

#[derive(Debug, Deserialize)]
struct CreateAccountRequest {
    name: String,
    account_type: AccountType,
    currency: String,
    opening_balance: Decimal,
}

#[derive(Debug, Deserialize)]
struct CreateCategoryRequest {
    name: String,
    kind: CategoryKind,
}

#[derive(Debug, Deserialize)]
struct CreateTransactionRequest {
    kind: TransactionKind,
    account_id: i64,
    category_id: i64,
    amount: Decimal,
    occurred_at: Option<DateTime<Utc>>,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct CreateTransferRequest {
    from_account_id: i64,
    to_account_id: i64,
    source_amount: Decimal,
    target_amount: Decimal,
    occurred_at: Option<DateTime<Utc>>,
    #[serde(default)]
    note: String,
}

#[derive(Debug, Deserialize)]
struct MonthlyQuery {
    year: Option<i32>,
    month: Option<u32>,
    currency: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BalanceQuery {
    currency: Option<String>,
}

#[derive(Clone)]
struct AppState {
    service: Arc<Mutex<BookkeepingService>>,
}

#[derive(Debug, Serialize)]
struct ApiErrorBody {
    error: String,
}

impl IntoResponse for KokuError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::AlreadyVoided => StatusCode::CONFLICT,
            Self::InvalidInput(_) | Self::CategoryKindMismatch { .. } => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            Self::Database(_) | Self::InvalidDecimal(_) | Self::Io(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        (
            status,
            Json(ApiErrorBody {
                error: self.to_string(),
            }),
        )
            .into_response()
    }
}

pub struct BookkeepingService {
    conn: Connection,
}

impl BookkeepingService {
    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    #[allow(dead_code)]
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }

    fn from_connection(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = 5000;

            CREATE TABLE IF NOT EXISTS accounts (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                name         TEXT NOT NULL UNIQUE,
                account_type TEXT NOT NULL CHECK (account_type IN ('asset', 'liability')),
                currency     TEXT NOT NULL,
                balance      TEXT NOT NULL,
                created_at   TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS categories (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                name       TEXT NOT NULL,
                kind       TEXT NOT NULL CHECK (kind IN ('expense', 'income')),
                created_at TEXT NOT NULL,
                UNIQUE(name, kind)
            );

            CREATE TABLE IF NOT EXISTS transactions (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                kind          TEXT NOT NULL CHECK (kind IN ('expense', 'income', 'transfer')),
                account_id    INTEGER NOT NULL REFERENCES accounts(id),
                to_account_id INTEGER REFERENCES accounts(id),
                category_id   INTEGER REFERENCES categories(id),
                amount        TEXT NOT NULL,
                target_amount TEXT,
                occurred_at   TEXT NOT NULL,
                note          TEXT NOT NULL DEFAULT '',
                voided_at     TEXT,
                CHECK (
                    (kind IN ('expense', 'income') AND category_id IS NOT NULL
                     AND to_account_id IS NULL AND target_amount IS NULL)
                    OR
                    (kind = 'transfer' AND category_id IS NULL
                     AND to_account_id IS NOT NULL AND target_amount IS NOT NULL)
                )
            );

            CREATE INDEX IF NOT EXISTS idx_transactions_month
                ON transactions(occurred_at, voided_at);
            CREATE INDEX IF NOT EXISTS idx_transactions_account
                ON transactions(account_id, to_account_id);
            "#,
        )?;
        Ok(Self { conn })
    }

    pub fn create_account(
        &mut self,
        name: impl Into<String>,
        account_type: AccountType,
        currency: impl Into<String>,
        opening_balance: Decimal,
    ) -> Result<Account> {
        let name = required_text(name.into(), "account name")?;
        let currency = normalize_currency(currency.into())?;
        let now = timestamp(Utc::now());
        self.conn.execute(
            "INSERT INTO accounts(name, account_type, currency, balance, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![name, account_type.as_str(), currency, decimal_to_db(opening_balance), now],
        )?;
        self.account(self.conn.last_insert_rowid())
    }

    pub fn create_category(
        &mut self,
        name: impl Into<String>,
        kind: CategoryKind,
    ) -> Result<Category> {
        let name = required_text(name.into(), "category name")?;
        self.conn.execute(
            "INSERT INTO categories(name, kind, created_at) VALUES (?1, ?2, ?3)",
            params![name, kind.as_str(), timestamp(Utc::now())],
        )?;
        self.category(self.conn.last_insert_rowid())
    }

    pub fn account(&self, id: i64) -> Result<Account> {
        let row = self
            .conn
            .query_row(
                "SELECT id, name, account_type, currency, balance FROM accounts WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or(KokuError::NotFound {
                entity: "account",
                id,
            })?;
        account_from_row(row)
    }

    pub fn accounts(&self) -> Result<Vec<Account>> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, account_type, currency, balance FROM accounts ORDER BY id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        rows.map(|row| account_from_row(row?)).collect()
    }

    pub fn balance_summary(&self, currency: &str) -> Result<BalanceSummary> {
        let currency = normalize_currency(currency.to_owned())?;
        let accounts = self.accounts()?;
        let mut total_assets = Decimal::ZERO;
        let mut total_liabilities = Decimal::ZERO;
        for account in accounts
            .into_iter()
            .filter(|account| account.currency == currency)
        {
            match account.account_type {
                AccountType::Asset => total_assets += account.balance,
                AccountType::Liability => total_liabilities += account.balance,
            }
        }
        Ok(BalanceSummary {
            currency,
            total_assets,
            total_liabilities,
            net_worth: total_assets - total_liabilities,
        })
    }

    pub fn is_empty(&self) -> Result<bool> {
        let count = self
            .conn
            .query_row("SELECT COUNT(*) FROM accounts", [], |row| {
                row.get::<_, i64>(0)
            })?;
        Ok(count == 0)
    }

    pub fn category(&self, id: i64) -> Result<Category> {
        let row = self
            .conn
            .query_row(
                "SELECT id, name, kind FROM categories WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or(KokuError::NotFound {
                entity: "category",
                id,
            })?;
        category_from_row(row)
    }

    pub fn categories(&self) -> Result<Vec<Category>> {
        let mut statement = self
            .conn
            .prepare("SELECT id, name, kind FROM categories ORDER BY kind, id")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| category_from_row(row?)).collect()
    }

    pub fn record_expense(
        &mut self,
        account_id: i64,
        category_id: i64,
        amount: Decimal,
        occurred_at: DateTime<Utc>,
        note: impl Into<String>,
    ) -> Result<Transaction> {
        self.record_categorized(
            TransactionKind::Expense,
            account_id,
            category_id,
            amount,
            occurred_at,
            note.into(),
        )
    }

    pub fn record_income(
        &mut self,
        account_id: i64,
        category_id: i64,
        amount: Decimal,
        occurred_at: DateTime<Utc>,
        note: impl Into<String>,
    ) -> Result<Transaction> {
        self.record_categorized(
            TransactionKind::Income,
            account_id,
            category_id,
            amount,
            occurred_at,
            note.into(),
        )
    }

    fn record_categorized(
        &mut self,
        kind: TransactionKind,
        account_id: i64,
        category_id: i64,
        amount: Decimal,
        occurred_at: DateTime<Utc>,
        note: String,
    ) -> Result<Transaction> {
        positive_amount(amount)?;
        let expected_category_kind = match kind {
            TransactionKind::Expense => CategoryKind::Expense,
            TransactionKind::Income => CategoryKind::Income,
            TransactionKind::Transfer => {
                return Err(KokuError::InvalidInput(
                    "categorized transactions cannot be transfers".to_owned(),
                ))
            }
        };

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let account = Self::account_in_tx(&tx, account_id)?;
        let category = Self::category_in_tx(&tx, category_id)?;
        if category.kind != expected_category_kind {
            return Err(KokuError::CategoryKindMismatch {
                expected: expected_category_kind.as_str(),
                actual: category.kind.as_str(),
            });
        }

        let new_balance = match kind {
            TransactionKind::Expense => account.balance - amount,
            TransactionKind::Income => account.balance + amount,
            TransactionKind::Transfer => unreachable!("validated above"),
        };
        Self::set_balance(&tx, account_id, new_balance)?;
        tx.execute(
            "INSERT INTO transactions(kind, account_id, category_id, amount, occurred_at, note) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![kind.as_str(), account_id, category_id, decimal_to_db(amount), timestamp(occurred_at), note],
        )?;
        let transaction_id = tx.last_insert_rowid();
        tx.commit()?;
        self.transaction(transaction_id)
    }

    pub fn record_transfer(
        &mut self,
        from_account_id: i64,
        to_account_id: i64,
        source_amount: Decimal,
        target_amount: Decimal,
        occurred_at: DateTime<Utc>,
        note: impl Into<String>,
    ) -> Result<Transaction> {
        if from_account_id == to_account_id {
            return Err(KokuError::InvalidInput(
                "source and target accounts must be different".to_owned(),
            ));
        }
        positive_amount(source_amount)?;
        positive_amount(target_amount)?;

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source = Self::account_in_tx(&tx, from_account_id)?;
        let target = Self::account_in_tx(&tx, to_account_id)?;
        if source.currency == target.currency && source_amount != target_amount {
            return Err(KokuError::InvalidInput(
                "same-currency transfers must use equal source and target amounts".to_owned(),
            ));
        }

        Self::set_balance(&tx, source.id, source.balance - source_amount)?;
        Self::set_balance(&tx, target.id, target.balance + target_amount)?;
        tx.execute(
            "INSERT INTO transactions(kind, account_id, to_account_id, amount, target_amount, occurred_at, note) VALUES ('transfer', ?1, ?2, ?3, ?4, ?5, ?6)",
            params![source.id, target.id, decimal_to_db(source_amount), decimal_to_db(target_amount), timestamp(occurred_at), note.into()],
        )?;
        let transaction_id = tx.last_insert_rowid();
        tx.commit()?;
        self.transaction(transaction_id)
    }

    pub fn void_transaction(&mut self, transaction_id: i64) -> Result<Transaction> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let transaction = Self::transaction_in_tx(&tx, transaction_id)?;
        if transaction.voided_at.is_some() {
            return Err(KokuError::AlreadyVoided);
        }

        let source = Self::account_in_tx(&tx, transaction.account_id)?;
        match transaction.kind {
            TransactionKind::Expense => {
                Self::set_balance(&tx, source.id, source.balance + transaction.amount)?;
            }
            TransactionKind::Income => {
                Self::set_balance(&tx, source.id, source.balance - transaction.amount)?;
            }
            TransactionKind::Transfer => {
                let target_id = transaction.to_account_id.ok_or_else(|| {
                    KokuError::InvalidInput("transfer is missing its target account".to_owned())
                })?;
                let target_amount = transaction.target_amount.ok_or_else(|| {
                    KokuError::InvalidInput("transfer is missing its target amount".to_owned())
                })?;
                let target = Self::account_in_tx(&tx, target_id)?;
                Self::set_balance(&tx, source.id, source.balance + transaction.amount)?;
                Self::set_balance(&tx, target.id, target.balance - target_amount)?;
            }
        }

        tx.execute(
            "UPDATE transactions SET voided_at = ?1 WHERE id = ?2 AND voided_at IS NULL",
            params![timestamp(Utc::now()), transaction_id],
        )?;
        tx.commit()?;
        self.transaction(transaction_id)
    }

    pub fn transaction(&self, id: i64) -> Result<Transaction> {
        let raw = self
            .conn
            .query_row(
                "SELECT id, kind, account_id, to_account_id, category_id, amount, target_amount, occurred_at, note, voided_at FROM transactions WHERE id = ?1",
                [id],
                transaction_row,
            )
            .optional()?
            .ok_or(KokuError::NotFound {
                entity: "transaction",
                id,
            })?;
        transaction_from_row(raw)
    }

    pub fn transactions(&self) -> Result<Vec<Transaction>> {
        let mut statement = self.conn.prepare(
            "SELECT id, kind, account_id, to_account_id, category_id, amount, target_amount, occurred_at, note, voided_at FROM transactions ORDER BY occurred_at DESC, id DESC LIMIT 500",
        )?;
        let rows = statement.query_map([], transaction_row)?;
        rows.map(|row| transaction_from_row(row?)).collect()
    }

    pub fn monthly_summary(&self, year: i32, month: u32, currency: &str) -> Result<MonthlySummary> {
        let cash_flow = self.cash_flow_summary(year, month, currency)?;
        Ok(MonthlySummary {
            year,
            month,
            currency: cash_flow.currency,
            total_income: cash_flow.total_income,
            total_expense: cash_flow.total_expense,
            net: cash_flow.retained,
            expenses_by_category: cash_flow
                .expense_destinations
                .into_iter()
                .map(|item| CategoryExpense {
                    category_id: item.category_id,
                    category_name: item.category_name,
                    amount: item.amount,
                    percentage: item.percentage,
                })
                .collect(),
        })
    }

    pub fn cash_flow_summary(
        &self,
        year: i32,
        month: u32,
        currency: &str,
    ) -> Result<CashFlowSummary> {
        let (start, end) = month_bounds(year, month)?;
        let currency = normalize_currency(currency.to_owned())?;
        let mut statement = self.conn.prepare(
            r#"
            SELECT t.kind, t.category_id, c.name, t.amount
            FROM transactions t
            JOIN accounts a ON a.id = t.account_id
            JOIN categories c ON c.id = t.category_id
            WHERE t.voided_at IS NULL
              AND t.kind IN ('expense', 'income')
              AND t.occurred_at >= ?1 AND t.occurred_at < ?2
              AND a.currency = ?3
            ORDER BY t.id
            "#,
        )?;
        let rows =
            statement.query_map(params![timestamp(start), timestamp(end), currency], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?;

        let mut total_income = Decimal::ZERO;
        let mut total_expense = Decimal::ZERO;
        let mut income_totals: BTreeMap<(i64, String), Decimal> = BTreeMap::new();
        let mut expense_totals: BTreeMap<(i64, String), Decimal> = BTreeMap::new();
        for row in rows {
            let (kind, category_id, category_name, amount_text) = row?;
            let amount = decimal_from_db(&amount_text)?;
            match TransactionKind::from_db(&kind)? {
                TransactionKind::Income => {
                    total_income += amount;
                    *income_totals
                        .entry((category_id, category_name))
                        .or_insert(Decimal::ZERO) += amount;
                }
                TransactionKind::Expense => {
                    total_expense += amount;
                    *expense_totals
                        .entry((category_id, category_name))
                        .or_insert(Decimal::ZERO) += amount;
                }
                TransactionKind::Transfer => {}
            }
        }

        let income_sources = cash_flow_items(income_totals, total_income);
        let expense_destinations = cash_flow_items(expense_totals, total_expense);
        Ok(CashFlowSummary {
            year,
            month,
            currency,
            total_income,
            total_expense,
            retained: total_income - total_expense,
            flow_total: total_income.max(total_expense),
            income_sources,
            expense_destinations,
        })
    }

    fn account_in_tx(tx: &SqlTransaction<'_>, id: i64) -> Result<Account> {
        let row = tx
            .query_row(
                "SELECT id, name, account_type, currency, balance FROM accounts WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or(KokuError::NotFound {
                entity: "account",
                id,
            })?;
        account_from_row(row)
    }

    fn category_in_tx(tx: &SqlTransaction<'_>, id: i64) -> Result<Category> {
        let row = tx
            .query_row(
                "SELECT id, name, kind FROM categories WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or(KokuError::NotFound {
                entity: "category",
                id,
            })?;
        category_from_row(row)
    }

    fn transaction_in_tx(tx: &SqlTransaction<'_>, id: i64) -> Result<Transaction> {
        let raw = tx
            .query_row(
                "SELECT id, kind, account_id, to_account_id, category_id, amount, target_amount, occurred_at, note, voided_at FROM transactions WHERE id = ?1",
                [id],
                transaction_row,
            )
            .optional()?
            .ok_or(KokuError::NotFound {
                entity: "transaction",
                id,
            })?;
        transaction_from_row(raw)
    }

    fn set_balance(tx: &SqlTransaction<'_>, account_id: i64, balance: Decimal) -> Result<()> {
        let changed = tx.execute(
            "UPDATE accounts SET balance = ?1 WHERE id = ?2",
            params![decimal_to_db(balance), account_id],
        )?;
        if changed != 1 {
            return Err(KokuError::NotFound {
                entity: "account",
                id: account_id,
            });
        }
        Ok(())
    }
}

type AccountRow = (i64, String, String, String, String);
type CategoryRow = (i64, String, String);
type TransactionRow = (
    i64,
    String,
    i64,
    Option<i64>,
    Option<i64>,
    String,
    Option<String>,
    String,
    String,
    Option<String>,
);

fn account_from_row(row: AccountRow) -> Result<Account> {
    Ok(Account {
        id: row.0,
        name: row.1,
        account_type: AccountType::from_db(&row.2)?,
        currency: row.3,
        balance: decimal_from_db(&row.4)?,
    })
}

fn category_from_row(row: CategoryRow) -> Result<Category> {
    Ok(Category {
        id: row.0,
        name: row.1,
        kind: CategoryKind::from_db(&row.2)?,
    })
}

fn transaction_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TransactionRow> {
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
    ))
}

fn transaction_from_row(row: TransactionRow) -> Result<Transaction> {
    Ok(Transaction {
        id: row.0,
        kind: TransactionKind::from_db(&row.1)?,
        account_id: row.2,
        to_account_id: row.3,
        category_id: row.4,
        amount: decimal_from_db(&row.5)?,
        target_amount: row.6.as_deref().map(decimal_from_db).transpose()?,
        occurred_at: parse_timestamp(&row.7)?,
        note: row.8,
        voided_at: row.9.as_deref().map(parse_timestamp).transpose()?,
    })
}

fn required_text(value: String, field: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(KokuError::InvalidInput(format!("{field} cannot be empty")));
    }
    Ok(trimmed.to_owned())
}

fn normalize_currency(value: String) -> Result<String> {
    let currency = required_text(value, "currency")?.to_uppercase();
    if currency.len() != 3 || !currency.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return Err(KokuError::InvalidInput(
            "currency must be a three-letter ISO-style code".to_owned(),
        ));
    }
    Ok(currency)
}

fn month_bounds(year: i32, month: u32) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    let start_date = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| KokuError::InvalidInput(format!("invalid year/month: {year}-{month}")))?;
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let end_date = NaiveDate::from_ymd_opt(next_year, next_month, 1).ok_or_else(|| {
        KokuError::InvalidInput(format!("invalid next month after {year}-{month}"))
    })?;
    let start = start_date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| KokuError::InvalidInput("invalid month start".to_owned()))?
        .and_utc();
    let end = end_date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| KokuError::InvalidInput("invalid month end".to_owned()))?
        .and_utc();
    Ok((start, end))
}

fn cash_flow_items(totals: BTreeMap<(i64, String), Decimal>, total: Decimal) -> Vec<CashFlowItem> {
    let hundred = Decimal::from(100_u32);
    let mut items: Vec<_> = totals
        .into_iter()
        .map(|((category_id, category_name), amount)| CashFlowItem {
            category_id,
            category_name,
            amount,
            percentage: if total.is_zero() {
                Decimal::ZERO
            } else {
                (amount / total * hundred).round_dp(2)
            },
        })
        .collect();
    items.sort_by_key(|item| std::cmp::Reverse(item.amount));
    items
}

fn positive_amount(amount: Decimal) -> Result<()> {
    if amount <= Decimal::ZERO {
        return Err(KokuError::InvalidInput(
            "transaction amount must be greater than zero".to_owned(),
        ));
    }
    Ok(())
}

fn decimal_to_db(value: Decimal) -> String {
    value.normalize().to_string()
}

fn decimal_from_db(value: &str) -> Result<Decimal> {
    Ok(Decimal::from_str(value)?)
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| KokuError::InvalidInput(format!("invalid timestamp in database: {error}")))
}

fn money(amount: Decimal, currency: &str) -> String {
    format!("{amount:.2} {currency}")
}

fn print_demo(accounts: &[Account], summary: &MonthlySummary) -> Result<()> {
    println!("\n╭──────────────────────────────────────────────────────╮");
    println!("│                  Koku · 本月财务概览                 │");
    println!("╰──────────────────────────────────────────────────────╯");
    println!("  月份       {:04}-{:02}", summary.year, summary.month);
    println!(
        "  总收入     {}",
        money(summary.total_income, &summary.currency)
    );
    println!(
        "  总支出     {}",
        money(summary.total_expense, &summary.currency)
    );
    println!("  净结余     {}", money(summary.net, &summary.currency));

    println!("\n┌──────────────── 账户实时余额 ────────────────┐");
    println!("  {:<16} {:<6} {:>18}", "账户", "类型", "余额");
    println!("  ────────────────────────────────────────────");
    for account in accounts {
        println!(
            "  {:<16} {:<6} {:>18}",
            account.name,
            account.account_type.label(),
            money(account.balance, &account.currency)
        );
    }
    println!("└──────────────────────────────────────────────┘");

    println!("\n┌──────────────── 支出分类明细 ────────────────┐");
    if summary.expenses_by_category.is_empty() {
        println!("  本月暂无支出");
    } else {
        println!("  {:<14} {:>18} {:>10}", "分类", "金额", "占比");
        println!("  ────────────────────────────────────────────");
        for item in &summary.expenses_by_category {
            println!(
                "  {:<14} {:>18} {:>9.2}%",
                item.category_name,
                money(item.amount, &summary.currency),
                item.percentage
            );
        }
    }
    println!("└──────────────────────────────────────────────┘");

    println!("\n可序列化统计 DTO（JSON）：");
    println!(
        "{}",
        serde_json::to_string_pretty(summary).map_err(|error| {
            KokuError::InvalidInput(format!("failed to serialize summary: {error}"))
        })?
    );
    Ok(())
}

fn run_demo() -> Result<()> {
    let mut service = BookkeepingService::in_memory()?;
    let alipay = service.create_account(
        "支付宝",
        AccountType::Asset,
        "CNY",
        Decimal::new(120_000, 2),
    )?;
    let cmb = service.create_account(
        "招商银行卡",
        AccountType::Asset,
        "CNY",
        Decimal::new(800_000, 2),
    )?;
    let _credit = service.create_account("信用卡", AccountType::Liability, "CNY", Decimal::ZERO)?;

    let salary = service.create_category("工资", CategoryKind::Income)?;
    let food = service.create_category("餐饮", CategoryKind::Expense)?;
    let transit = service.create_category("交通", CategoryKind::Expense)?;
    let now = Utc::now();

    service.record_income(cmb.id, salary.id, Decimal::new(850_000, 2), now, "八月工资")?;
    service.record_expense(alipay.id, food.id, Decimal::new(6_850, 2), now, "晚餐")?;
    service.record_transfer(
        cmb.id,
        alipay.id,
        Decimal::new(100_000, 2),
        Decimal::new(100_000, 2),
        now,
        "日常消费金",
    )?;
    service.record_expense(alipay.id, transit.id, Decimal::new(1_200, 2), now, "地铁")?;

    let cancelled = service.record_expense(
        alipay.id,
        food.id,
        Decimal::new(2_580, 2),
        now,
        "误记的午餐",
    )?;
    service.void_transaction(cancelled.id)?;

    let summary = service.monthly_summary(now.year(), now.month(), "CNY")?;
    print_demo(&service.accounts()?, &summary)
}

fn seed_demo_data(service: &mut BookkeepingService) -> Result<()> {
    if !service.is_empty()? {
        return Ok(());
    }

    let alipay = service.create_account(
        "支付宝",
        AccountType::Asset,
        "CNY",
        Decimal::new(328_000, 2),
    )?;
    let cmb = service.create_account(
        "招商银行卡",
        AccountType::Asset,
        "CNY",
        Decimal::new(2_856_000, 2),
    )?;
    let cash = service.create_account(
        "现金钱包",
        AccountType::Asset,
        "CNY",
        Decimal::new(56_000, 2),
    )?;
    let credit = service.create_account(
        "信用卡",
        AccountType::Liability,
        "CNY",
        Decimal::new(126_000, 2),
    )?;

    let salary = service.create_category("工资", CategoryKind::Income)?;
    let side_job = service.create_category("副业", CategoryKind::Income)?;
    let food = service.create_category("餐饮", CategoryKind::Expense)?;
    let transit = service.create_category("交通", CategoryKind::Expense)?;
    let shopping = service.create_category("购物", CategoryKind::Expense)?;
    let home = service.create_category("居家", CategoryKind::Expense)?;
    let entertainment = service.create_category("娱乐", CategoryKind::Expense)?;

    let now = Utc::now();
    service.record_income(
        cmb.id,
        salary.id,
        Decimal::new(1_280_000, 2),
        now,
        "本月工资",
    )?;
    service.record_income(
        alipay.id,
        side_job.id,
        Decimal::new(168_000, 2),
        now,
        "设计项目尾款",
    )?;
    service.record_expense(alipay.id, food.id, Decimal::new(6_850, 2), now, "梧桐小馆")?;
    service.record_expense(
        alipay.id,
        transit.id,
        Decimal::new(1_200, 2),
        now,
        "地铁通勤",
    )?;
    service.record_expense(cmb.id, home.id, Decimal::new(280_000, 2), now, "房租")?;
    service.record_expense(
        credit.id,
        shopping.id,
        Decimal::new(38_900, 2),
        now,
        "生活用品",
    )?;
    service.record_expense(cash.id, food.id, Decimal::new(2_400, 2), now, "咖啡")?;
    service.record_expense(
        alipay.id,
        entertainment.id,
        Decimal::new(4_500, 2),
        now,
        "电影",
    )?;
    service.record_transfer(
        cmb.id,
        alipay.id,
        Decimal::new(100_000, 2),
        Decimal::new(100_000, 2),
        now,
        "日常消费金",
    )?;
    Ok(())
}

fn lock_service(state: &AppState) -> Result<MutexGuard<'_, BookkeepingService>> {
    state
        .service
        .lock()
        .map_err(|_| KokuError::InvalidInput("bookkeeping service lock was poisoned".to_owned()))
}

async fn api_health() -> Json<ApiResponse<serde_json::Value>> {
    Json(ApiResponse::new(serde_json::json!({
        "status": "ok",
        "service": "koku-api"
    })))
}

async fn api_accounts(State(state): State<AppState>) -> Result<Json<ApiResponse<Vec<Account>>>> {
    let accounts = lock_service(&state)?.accounts()?;
    Ok(Json(ApiResponse::new(accounts)))
}

async fn api_create_account(
    State(state): State<AppState>,
    Json(request): Json<CreateAccountRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Account>>)> {
    let account = lock_service(&state)?.create_account(
        request.name,
        request.account_type,
        request.currency,
        request.opening_balance,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(account))))
}

async fn api_categories(State(state): State<AppState>) -> Result<Json<ApiResponse<Vec<Category>>>> {
    let categories = lock_service(&state)?.categories()?;
    Ok(Json(ApiResponse::new(categories)))
}

async fn api_create_category(
    State(state): State<AppState>,
    Json(request): Json<CreateCategoryRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Category>>)> {
    let category = lock_service(&state)?.create_category(request.name, request.kind)?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(category))))
}

async fn api_transactions(
    State(state): State<AppState>,
) -> Result<Json<ApiResponse<Vec<Transaction>>>> {
    let transactions = lock_service(&state)?.transactions()?;
    Ok(Json(ApiResponse::new(transactions)))
}

async fn api_create_transaction(
    State(state): State<AppState>,
    Json(request): Json<CreateTransactionRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let occurred_at = request.occurred_at.unwrap_or_else(Utc::now);
    let mut service = lock_service(&state)?;
    let transaction = match request.kind {
        TransactionKind::Expense => service.record_expense(
            request.account_id,
            request.category_id,
            request.amount,
            occurred_at,
            request.note,
        )?,
        TransactionKind::Income => service.record_income(
            request.account_id,
            request.category_id,
            request.amount,
            occurred_at,
            request.note,
        )?,
        TransactionKind::Transfer => {
            return Err(KokuError::InvalidInput(
                "use /api/transfers for transfer transactions".to_owned(),
            ))
        }
    };
    Ok((StatusCode::CREATED, Json(ApiResponse::new(transaction))))
}

async fn api_create_transfer(
    State(state): State<AppState>,
    Json(request): Json<CreateTransferRequest>,
) -> Result<(StatusCode, Json<ApiResponse<Transaction>>)> {
    let transaction = lock_service(&state)?.record_transfer(
        request.from_account_id,
        request.to_account_id,
        request.source_amount,
        request.target_amount,
        request.occurred_at.unwrap_or_else(Utc::now),
        request.note,
    )?;
    Ok((StatusCode::CREATED, Json(ApiResponse::new(transaction))))
}

async fn api_void_transaction(
    State(state): State<AppState>,
    AxumPath(transaction_id): AxumPath<i64>,
) -> Result<Json<ApiResponse<Transaction>>> {
    let transaction = lock_service(&state)?.void_transaction(transaction_id)?;
    Ok(Json(ApiResponse::new(transaction)))
}

async fn api_monthly_summary(
    State(state): State<AppState>,
    Query(query): Query<MonthlyQuery>,
) -> Result<Json<ApiResponse<MonthlySummary>>> {
    let now = Utc::now();
    let summary = lock_service(&state)?.monthly_summary(
        query.year.unwrap_or_else(|| now.year()),
        query.month.unwrap_or_else(|| now.month()),
        query.currency.as_deref().unwrap_or("CNY"),
    )?;
    Ok(Json(ApiResponse::new(summary)))
}

async fn api_cash_flow_summary(
    State(state): State<AppState>,
    Query(query): Query<MonthlyQuery>,
) -> Result<Json<ApiResponse<CashFlowSummary>>> {
    let now = Utc::now();
    let summary = lock_service(&state)?.cash_flow_summary(
        query.year.unwrap_or_else(|| now.year()),
        query.month.unwrap_or_else(|| now.month()),
        query.currency.as_deref().unwrap_or("CNY"),
    )?;
    Ok(Json(ApiResponse::new(summary)))
}

async fn api_balance_summary(
    State(state): State<AppState>,
    Query(query): Query<BalanceQuery>,
) -> Result<Json<ApiResponse<BalanceSummary>>> {
    let summary =
        lock_service(&state)?.balance_summary(query.currency.as_deref().unwrap_or("CNY"))?;
    Ok(Json(ApiResponse::new(summary)))
}

fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(api_health))
        .route("/api/accounts", get(api_accounts).post(api_create_account))
        .route(
            "/api/categories",
            get(api_categories).post(api_create_category),
        )
        .route(
            "/api/transactions",
            get(api_transactions).post(api_create_transaction),
        )
        .route("/api/transfers", post(api_create_transfer))
        .route(
            "/api/transactions/{transaction_id}",
            delete(api_void_transaction),
        )
        .route("/api/summary/monthly", get(api_monthly_summary))
        .route("/api/summary/cash-flow", get(api_cash_flow_summary))
        .route("/api/summary/balance", get(api_balance_summary))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state)
}

async fn run_server() -> Result<()> {
    let database_path = std::env::var("KOKU_DB_PATH").unwrap_or_else(|_| "data/koku.db".to_owned());
    if let Some(parent) = Path::new(&database_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut service = BookkeepingService::open(&database_path)?;
    seed_demo_data(&mut service)?;

    let port = std::env::var("KOKU_PORT")
        .unwrap_or_else(|_| "8080".to_owned())
        .parse::<u16>()
        .map_err(|error| KokuError::InvalidInput(format!("invalid KOKU_PORT: {error}")))?;
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Koku API is listening on http://{address}");

    let state = AppState {
        service: Arc::new(Mutex::new(service)),
    };
    axum::serve(listener, api_router(state))
        .with_graceful_shutdown(async {
            if let Err(error) = tokio::signal::ctrl_c().await {
                eprintln!("failed to listen for shutdown signal: {error}");
            }
        })
        .await?;
    Ok(())
}

#[tokio::main]
async fn main() {
    let result = if std::env::args().any(|argument| argument == "--demo") {
        run_demo()
    } else {
        run_server().await
    };
    if let Err(error) = result {
        eprintln!("Koku failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_service() -> Result<BookkeepingService> {
        BookkeepingService::in_memory()
    }

    #[test]
    fn transfer_updates_both_balances_exactly_and_is_atomic() -> Result<()> {
        let mut service = test_service()?;
        let source = service.create_account(
            "Source",
            AccountType::Asset,
            "CNY",
            Decimal::from_str("1000.10")?,
        )?;
        let target = service.create_account(
            "Target",
            AccountType::Asset,
            "CNY",
            Decimal::from_str("10.20")?,
        )?;

        service.record_transfer(
            source.id,
            target.id,
            Decimal::from_str("333.33")?,
            Decimal::from_str("333.33")?,
            Utc::now(),
            "exact transfer",
        )?;
        assert_eq!(
            service.account(source.id)?.balance,
            Decimal::from_str("666.77")?
        );
        assert_eq!(
            service.account(target.id)?.balance,
            Decimal::from_str("343.53")?
        );

        let before = service.account(source.id)?.balance;
        let result = service.record_transfer(
            source.id,
            999_999,
            Decimal::ONE,
            Decimal::ONE,
            Utc::now(),
            "must roll back",
        );
        assert!(matches!(result, Err(KokuError::NotFound { .. })));
        assert_eq!(service.account(source.id)?.balance, before);
        Ok(())
    }

    #[test]
    fn void_transfer_restores_both_accounts_once() -> Result<()> {
        let mut service = test_service()?;
        let source =
            service.create_account("Wallet", AccountType::Asset, "CNY", Decimal::from(500_u32))?;
        let target =
            service.create_account("Bank", AccountType::Asset, "CNY", Decimal::from(100_u32))?;
        let transfer = service.record_transfer(
            source.id,
            target.id,
            Decimal::from_str("88.88")?,
            Decimal::from_str("88.88")?,
            Utc::now(),
            "temporary",
        )?;

        let voided = service.void_transaction(transfer.id)?;
        assert!(voided.voided_at.is_some());
        assert_eq!(service.account(source.id)?.balance, Decimal::from(500_u32));
        assert_eq!(service.account(target.id)?.balance, Decimal::from(100_u32));
        assert!(matches!(
            service.void_transaction(transfer.id),
            Err(KokuError::AlreadyVoided)
        ));
        assert_eq!(service.account(source.id)?.balance, Decimal::from(500_u32));
        Ok(())
    }

    #[test]
    fn categorized_transactions_and_void_are_reflected_in_monthly_stats() -> Result<()> {
        let mut service = test_service()?;
        let account =
            service.create_account("Checking", AccountType::Asset, "CNY", Decimal::ZERO)?;
        let salary = service.create_category("Salary", CategoryKind::Income)?;
        let food = service.create_category("Food", CategoryKind::Expense)?;
        let transit = service.create_category("Transit", CategoryKind::Expense)?;
        let at = NaiveDate::from_ymd_opt(2026, 8, 15)
            .and_then(|date| date.and_hms_opt(12, 0, 0))
            .ok_or_else(|| KokuError::InvalidInput("invalid test date".to_owned()))?
            .and_utc();

        service.record_income(account.id, salary.id, Decimal::from(1000_u32), at, "salary")?;
        service.record_expense(account.id, food.id, Decimal::from(150_u32), at, "food")?;
        service.record_expense(account.id, transit.id, Decimal::from(50_u32), at, "transit")?;
        let mistake =
            service.record_expense(account.id, food.id, Decimal::from(25_u32), at, "mistake")?;
        service.void_transaction(mistake.id)?;

        let summary = service.monthly_summary(2026, 8, "cny")?;
        assert_eq!(summary.total_income, Decimal::from(1000_u32));
        assert_eq!(summary.total_expense, Decimal::from(200_u32));
        assert_eq!(summary.net, Decimal::from(800_u32));
        assert_eq!(summary.expenses_by_category.len(), 2);
        assert_eq!(summary.expenses_by_category[0].category_name, "Food");
        assert_eq!(
            summary.expenses_by_category[0].percentage,
            Decimal::from(75_u32)
        );
        assert_eq!(service.account(account.id)?.balance, Decimal::from(800_u32));

        let cash_flow = service.cash_flow_summary(2026, 8, "CNY")?;
        assert_eq!(cash_flow.flow_total, Decimal::from(1000_u32));
        assert_eq!(cash_flow.retained, Decimal::from(800_u32));
        assert_eq!(cash_flow.income_sources.len(), 1);
        assert_eq!(cash_flow.income_sources[0].category_name, "Salary");
        assert_eq!(
            cash_flow.income_sources[0].percentage,
            Decimal::from(100_u32)
        );
        assert_eq!(cash_flow.expense_destinations.len(), 2);
        assert_eq!(cash_flow.expense_destinations[0].category_name, "Food");
        Ok(())
    }

    #[test]
    fn cross_currency_transfer_uses_explicit_target_amounts() -> Result<()> {
        let mut service = test_service()?;
        let cny = service.create_account(
            "CNY account",
            AccountType::Asset,
            "CNY",
            Decimal::from(1000_u32),
        )?;
        let usd =
            service.create_account("USD account", AccountType::Asset, "USD", Decimal::ZERO)?;
        service.record_transfer(
            cny.id,
            usd.id,
            Decimal::from(720_u32),
            Decimal::from(100_u32),
            Utc::now(),
            "explicit FX conversion",
        )?;
        assert_eq!(service.account(cny.id)?.balance, Decimal::from(280_u32));
        assert_eq!(service.account(usd.id)?.balance, Decimal::from(100_u32));
        Ok(())
    }
}
