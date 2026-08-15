//! 领域类型：账户/分类/交易枚举、内置分类与对外 DTO。

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::{KokuError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountType {
    Asset,
    Liability,
}

impl AccountType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asset => "asset",
            Self::Liability => "liability",
        }
    }

    pub fn from_db(value: &str) -> Result<Self> {
        match value {
            "asset" => Ok(Self::Asset),
            "liability" => Ok(Self::Liability),
            other => Err(KokuError::InvalidInput(format!(
                "unknown account type in database: {other}"
            ))),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Asset => "资产",
            Self::Liability => "负债",
        }
    }

    pub fn apply_inflow(self, balance: Decimal, amount: Decimal) -> Decimal {
        match self {
            Self::Asset => balance + amount,
            Self::Liability => balance - amount,
        }
    }

    pub fn apply_outflow(self, balance: Decimal, amount: Decimal) -> Decimal {
        match self {
            Self::Asset => balance - amount,
            Self::Liability => balance + amount,
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
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Expense => "expense",
            Self::Income => "income",
        }
    }

    pub fn from_db(value: &str) -> Result<Self> {
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
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Expense => "expense",
            Self::Income => "income",
            Self::Transfer => "transfer",
        }
    }

    pub fn from_db(value: &str) -> Result<Self> {
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

/// 开箱即用的 28 个收入/支出分类。
pub const DEFAULT_CATEGORIES: &[(&str, CategoryKind)] = &[
    ("工资", CategoryKind::Income),
    ("奖金", CategoryKind::Income),
    ("副业", CategoryKind::Income),
    ("投资收益", CategoryKind::Income),
    ("利息", CategoryKind::Income),
    ("报销", CategoryKind::Income),
    ("礼金", CategoryKind::Income),
    ("退款", CategoryKind::Income),
    ("其他收入", CategoryKind::Income),
    ("餐饮", CategoryKind::Expense),
    ("交通", CategoryKind::Expense),
    ("购物", CategoryKind::Expense),
    ("居家", CategoryKind::Expense),
    ("娱乐", CategoryKind::Expense),
    ("医疗保健", CategoryKind::Expense),
    ("教育", CategoryKind::Expense),
    ("旅行", CategoryKind::Expense),
    ("通讯", CategoryKind::Expense),
    ("水电燃气", CategoryKind::Expense),
    ("住房", CategoryKind::Expense),
    ("保险", CategoryKind::Expense),
    ("数字订阅", CategoryKind::Expense),
    ("运动健身", CategoryKind::Expense),
    ("宠物", CategoryKind::Expense),
    ("人情往来", CategoryKind::Expense),
    ("家庭", CategoryKind::Expense),
    ("税费", CategoryKind::Expense),
    ("其他支出", CategoryKind::Expense),
];

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
    pub currency: String,
    pub settled_amount: Decimal,
    pub target_amount: Option<Decimal>,
    pub target_currency: Option<String>,
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
