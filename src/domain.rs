//! 领域类型：账户/分类/交易枚举、内置分类与对外 DTO。

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::{KokuError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountType {
    /// 零钱：日常收支账户
    Cash,
    /// 信用：信用卡/花呗等，余额 = 未还欠款（负债）
    Credit,
    /// 储蓄：存款与定期
    Savings,
    /// 股票：证券账户（按市值/成本记账）
    Stock,
}

impl AccountType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cash => "cash",
            Self::Credit => "credit",
            Self::Savings => "savings",
            Self::Stock => "stock",
        }
    }

    pub fn from_db(value: &str) -> Result<Self> {
        match value {
            "cash" => Ok(Self::Cash),
            "credit" => Ok(Self::Credit),
            "savings" => Ok(Self::Savings),
            "stock" => Ok(Self::Stock),
            other => Err(KokuError::InvalidInput(format!(
                "unknown account type in database: {other}"
            ))),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cash => "零钱",
            Self::Credit => "信用",
            Self::Savings => "储蓄",
            Self::Stock => "股票",
        }
    }

    /// 只有信用账户是负债，余额方向相反。
    pub fn is_liability(self) -> bool {
        matches!(self, Self::Credit)
    }

    pub fn apply_inflow(self, balance: Decimal, amount: Decimal) -> Decimal {
        if self.is_liability() {
            balance - amount
        } else {
            balance + amount
        }
    }

    pub fn apply_outflow(self, balance: Decimal, amount: Decimal) -> Decimal {
        if self.is_liability() {
            balance + amount
        } else {
            balance - amount
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
    /// 借款资金移动（借出/借入/还款），不计入收支统计
    Loan,
}

impl TransactionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Expense => "expense",
            Self::Income => "income",
            Self::Transfer => "transfer",
            Self::Loan => "loan",
        }
    }

    pub fn from_db(value: &str) -> Result<Self> {
        match value {
            "expense" => Ok(Self::Expense),
            "income" => Ok(Self::Income),
            "transfer" => Ok(Self::Transfer),
            "loan" => Ok(Self::Loan),
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
    /// 定期利率（百分比，如 2.10 = 2.10%）；仅定期存款账户有值
    pub interest_rate: Option<Decimal>,
    /// 定期到期日；仅定期存款账户有值
    pub maturity_at: Option<DateTime<Utc>>,
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
    /// 关联的借款记录（借出/借入/还款流水）
    pub loan_id: Option<i64>,
    /// 待报销标记时间
    pub reimbursable_at: Option<DateTime<Utc>>,
    /// 全部报销完成时间
    pub reimbursed_at: Option<DateTime<Utc>>,
    /// 累计已报销金额（原币种）
    pub reimbursed_amount: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoanType {
    /// 借出（应收）
    Lend,
    /// 借入（应付）
    Borrow,
}

impl LoanType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lend => "lend",
            Self::Borrow => "borrow",
        }
    }

    pub fn from_db(value: &str) -> Result<Self> {
        match value {
            "lend" => Ok(Self::Lend),
            "borrow" => Ok(Self::Borrow),
            other => Err(KokuError::InvalidInput(format!(
                "unknown loan type in database: {other}"
            ))),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Lend => "借出",
            Self::Borrow => "借入",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Loan {
    pub id: i64,
    pub loan_type: LoanType,
    pub counterparty: String,
    pub currency: String,
    pub principal: Decimal,
    pub outstanding: Decimal,
    /// 首笔资金进出的账户
    pub account_id: i64,
    pub opened_at: DateTime<Utc>,
    pub note: String,
    pub closed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepositSettlement {
    pub interest: Decimal,
    pub transfer: Transaction,
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
