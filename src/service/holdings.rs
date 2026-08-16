//! 股票持仓：买入/卖出复合操作，同时影响账户现金余额与持仓。
//!
//! 现金金额统一四舍五入到 2 位小数；持仓股数保留完整精度（可含小数股）。

use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use rust_decimal::Decimal;

use super::*;
use crate::domain::{AccountType, Holding, Transaction};
use crate::error::{KokuError, Result};
use crate::service::BookkeepingService;

impl BookkeepingService {
    pub fn holdings(&self) -> Result<Vec<Holding>> {
        let mut statement = self.conn.prepare(
            "SELECT id, account_id, symbol, shares, cost_basis, last_price
             FROM holdings ORDER BY symbol, account_id",
        )?;
        let rows = statement.query_map([], holding_row)?;
        rows.map(|row| holding_from_row(row?)).collect()
    }

    pub fn holding(&self, id: i64) -> Result<Holding> {
        let row = self
            .conn
            .query_row(
                "SELECT id, account_id, symbol, shares, cost_basis, last_price FROM holdings WHERE id = ?1",
                [id],
                holding_row,
            )
            .optional()?
            .ok_or(KokuError::NotFound {
                entity: "holding",
                id,
            })?;
        holding_from_row(row)
    }

    pub fn set_holding_price(&mut self, holding_id: i64, price: Decimal) -> Result<Holding> {
        positive_amount(price)?;
        let changed = self.conn.execute(
            "UPDATE holdings SET last_price = ?1, updated_at = ?2 WHERE id = ?3",
            params![decimal_to_db(price), timestamp(Utc::now()), holding_id],
        )?;
        if changed != 1 {
            return Err(KokuError::NotFound {
                entity: "holding",
                id: holding_id,
            });
        }
        self.holding(holding_id)
    }

    /// 买入：现金流出 + 增持。`price` 为每股价格（账户币种）。
    #[allow(clippy::too_many_arguments)]
    pub fn buy_stock(
        &mut self,
        account_id: i64,
        symbol: String,
        shares: Decimal,
        price: Decimal,
        occurred_at: DateTime<Utc>,
        note: String,
    ) -> Result<Transaction> {
        positive_amount(shares)?;
        positive_amount(price)?;
        let symbol = normalize_symbol(&symbol)?;
        let cash = (shares * price).round_dp(2);
        let description = format!("买入 {symbol} {shares} 股 @ {price}");

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let account = Self::account_in_tx(&tx, account_id)?;
        ensure_stock_account(&account)?;
        Self::set_balance(
            &tx,
            account_id,
            account.account_type.apply_outflow(account.balance, cash),
        )?;

        let (new_shares, new_cost) = existing_position(&tx, account_id, &symbol)?
            .map(|(shares0, cost0)| (shares0 + shares, cost0 + cash))
            .unwrap_or((shares, cash));
        tx.execute(
            "INSERT INTO holdings(account_id, symbol, shares, cost_basis, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(account_id, symbol)
             DO UPDATE SET shares = excluded.shares, cost_basis = excluded.cost_basis, updated_at = excluded.updated_at",
            params![account_id, symbol, decimal_to_db(new_shares), decimal_to_db(new_cost), timestamp(Utc::now())],
        )?;
        insert_trade_transaction(&tx, account_id, -cash, &account.currency, occurred_at, description, note)?;
        let transaction_id = tx.last_insert_rowid();
        tx.commit()?;
        self.transaction(transaction_id)
    }

    /// 卖出：现金流入 + 减持（摊薄成本法），清仓时删除持仓。`price` 为每股价格。
    #[allow(clippy::too_many_arguments)]
    pub fn sell_stock(
        &mut self,
        account_id: i64,
        symbol: String,
        shares: Decimal,
        price: Decimal,
        occurred_at: DateTime<Utc>,
        note: String,
    ) -> Result<Transaction> {
        positive_amount(shares)?;
        positive_amount(price)?;
        let symbol = normalize_symbol(&symbol)?;
        let cash = (shares * price).round_dp(2);
        let description = format!("卖出 {symbol} {shares} 股 @ {price}");

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let account = Self::account_in_tx(&tx, account_id)?;
        ensure_stock_account(&account)?;
        let (shares0, cost0) = existing_position(&tx, account_id, &symbol)?.ok_or_else(|| {
            KokuError::InvalidInput(format!("no holding for symbol {symbol}"))
        })?;
        if shares0 < shares {
            return Err(KokuError::InvalidInput(format!(
                "not enough shares of {symbol}: holding {shares0}, selling {shares}"
            )));
        }
        Self::set_balance(
            &tx,
            account_id,
            account.account_type.apply_inflow(account.balance, cash),
        )?;

        let new_shares = shares0 - shares;
        if new_shares.is_zero() {
            tx.execute(
                "DELETE FROM holdings WHERE account_id = ?1 AND symbol = ?2",
                params![account_id, symbol],
            )?;
        } else {
            let average_cost = if shares0.is_zero() {
                Decimal::ZERO
            } else {
                cost0 / shares0
            };
            let new_cost = cost0 - (average_cost * shares).round_dp(2);
            tx.execute(
                "UPDATE holdings SET shares = ?1, cost_basis = ?2, updated_at = ?3
                 WHERE account_id = ?4 AND symbol = ?5",
                params![
                    decimal_to_db(new_shares),
                    decimal_to_db(new_cost),
                    timestamp(Utc::now()),
                    account_id,
                    symbol
                ],
            )?;
        }
        insert_trade_transaction(&tx, account_id, cash, &account.currency, occurred_at, description, note)?;
        let transaction_id = tx.last_insert_rowid();
        tx.commit()?;
        self.transaction(transaction_id)
    }
}

fn ensure_stock_account(account: &crate::domain::Account) -> Result<()> {
    if account.account_type != AccountType::Stock {
        return Err(KokuError::InvalidInput(
            "stock trades require a stock account".to_owned(),
        ));
    }
    Ok(())
}

fn existing_position(
    tx: &rusqlite::Transaction<'_>,
    account_id: i64,
    symbol: &str,
) -> Result<Option<(Decimal, Decimal)>> {
    let row = tx
        .query_row(
            "SELECT shares, cost_basis FROM holdings WHERE account_id = ?1 AND symbol = ?2",
            params![account_id, symbol],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    row.map(|(shares, cost)| Ok((decimal_from_db(&shares)?, decimal_from_db(&cost)?)))
        .transpose()
}

fn insert_trade_transaction(
    tx: &rusqlite::Transaction<'_>,
    account_id: i64,
    signed_cash: Decimal,
    currency: &str,
    occurred_at: DateTime<Utc>,
    description: String,
    note: String,
) -> Result<()> {
    let combined = if note.trim().is_empty() {
        description
    } else {
        format!("{description} · {note}")
    };
    tx.execute(
        "INSERT INTO transactions(kind, account_id, amount, currency, settled_amount, occurred_at, note)
         VALUES ('trade', ?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            account_id,
            decimal_to_db(signed_cash),
            currency,
            decimal_to_db(signed_cash),
            timestamp(occurred_at),
            combined
        ],
    )?;
    Ok(())
}

fn normalize_symbol(value: &str) -> Result<String> {
    let symbol = value.trim().to_uppercase();
    if symbol.is_empty() {
        return Err(KokuError::InvalidInput(
            "stock symbol cannot be empty".to_owned(),
        ));
    }
    if symbol.chars().count() > 16 {
        return Err(KokuError::InvalidInput(
            "stock symbol must be 16 characters or fewer".to_owned(),
        ));
    }
    if !symbol
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
    {
        return Err(KokuError::InvalidInput(
            "stock symbol must be alphanumeric (with . or -)".to_owned(),
        ));
    }
    Ok(symbol)
}

type HoldingRow = (i64, i64, String, String, String, Option<String>);

fn holding_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HoldingRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ))
}

fn holding_from_row(row: HoldingRow) -> Result<Holding> {
    let shares = decimal_from_db(&row.3)?;
    let cost_basis = decimal_from_db(&row.4)?;
    let last_price = row.5.as_deref().map(decimal_from_db).transpose()?;
    let average_cost = if shares.is_zero() {
        Decimal::ZERO
    } else {
        cost_basis / shares
    };
    Ok(Holding {
        id: row.0,
        account_id: row.1,
        symbol: row.2,
        shares,
        cost_basis,
        last_price,
        average_cost,
    })
}
