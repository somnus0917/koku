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
    /// 余额调整（修正账户余额），amount 为带符号增量，不计入收支统计
    Adjustment,
    /// 股票买卖（现金进出），amount 为带符号现金额，不计入收支统计
    Trade,
    /// 定期存取（转入/到期转回），amount 为带符号现金额，不计入收支统计
    Deposit,
}

impl TransactionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Expense => "expense",
            Self::Income => "income",
            Self::Transfer => "transfer",
            Self::Loan => "loan",
            Self::Adjustment => "adjustment",
            Self::Trade => "trade",
            Self::Deposit => "deposit",
        }
    }

    pub fn from_db(value: &str) -> Result<Self> {
        match value {
            "expense" => Ok(Self::Expense),
            "income" => Ok(Self::Income),
            "transfer" => Ok(Self::Transfer),
            "loan" => Ok(Self::Loan),
            "adjustment" => Ok(Self::Adjustment),
            "trade" => Ok(Self::Trade),
            "deposit" => Ok(Self::Deposit),
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
    /// 信用额度；仅信用账户有值
    pub credit_limit: Option<Decimal>,
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
    /// 是否已挂有小票/发票附件
    pub has_receipt: bool,
    /// 关联标签（标签名，按创建顺序）
    #[serde(default)]
    pub tags: Vec<String>,
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
    /// 约定还款/到期日（可选，用于到期提醒）
    pub due_at: Option<DateTime<Utc>>,
}

/// 一笔定期存款（独立实体，不再是一个账户）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deposit {
    pub id: i64,
    pub source_account_id: i64,
    pub amount: Decimal,
    pub currency: String,
    /// 年化利率（百分比，如 2.10 = 2.10%）
    pub rate: Decimal,
    pub term_days: u32,
    pub opened_at: DateTime<Utc>,
    pub maturity_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
    pub note: String,
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
    /// 该分类当月的预算上限；未设置时不序列化。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_limit: Option<Decimal>,
}

/// 某分类在某自然月的预算上限。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    pub category_id: i64,
    pub category_name: String,
    pub category_kind: CategoryKind,
    pub year: i32,
    pub month: u32,
    pub limit_amount: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecurrenceFrequency {
    /// 每月一次（按 next_due_at 的日历月 +1，月末自动夹取）
    Monthly,
    /// 每周一次（next_due_at + 7 天）
    Weekly,
}

impl RecurrenceFrequency {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Monthly => "monthly",
            Self::Weekly => "weekly",
        }
    }

    pub fn from_db(value: &str) -> Result<Self> {
        match value {
            "monthly" => Ok(Self::Monthly),
            "weekly" => Ok(Self::Weekly),
            other => Err(KokuError::InvalidInput(format!(
                "unknown recurrence frequency in database: {other}"
            ))),
        }
    }
}

/// 标签（跨类目聚合用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub id: i64,
    pub name: String,
}

/// 股票账户的一只持仓：股数、总成本、可选市价与摊薄成本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holding {
    pub id: i64,
    pub account_id: i64,
    pub symbol: String,
    pub shares: Decimal,
    pub cost_basis: Decimal,
    pub last_price: Option<Decimal>,
    pub average_cost: Decimal,
}

/// 交易的小票/发票附件元数据（不含图片字节）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub transaction_id: i64,
    pub content_type: String,
    pub byte_length: usize,
    pub created_at: DateTime<Utc>,
}

/// 周期交易模板：到点自动生成一笔收入/支出并推进下一次生成时间。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecurringRule {
    pub id: i64,
    pub kind: TransactionKind,
    pub account_id: i64,
    pub category_id: i64,
    pub amount: Decimal,
    pub note: String,
    pub frequency: RecurrenceFrequency,
    pub next_due_at: DateTime<Utc>,
    pub paused_at: Option<DateTime<Utc>>,
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

/// 跨月趋势中的一个自然月点：收入/支出/结余均已折算到显示币种。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonthlyTrendPoint {
    pub year: i32,
    pub month: u32,
    pub total_income: Decimal,
    pub total_expense: Decimal,
    pub net: Decimal,
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

/// 标签汇总：同时带有全部指定标签的收支流水，按分类聚合并折算到显示币种。
/// `year`/`month` 为 `None` 时统计全部历史。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagSummary {
    pub tags: Vec<String>,
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub currency: String,
    pub total_income: Decimal,
    pub total_expense: Decimal,
    pub retained: Decimal,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateQuote {
    pub from: String,
    pub to: String,
    /// 参考汇率：1 from = rate to
    pub rate: Decimal,
    /// 汇率生效日期（YYYY-MM-DD，来自数据源）
    pub date: String,
    pub source: String,
    /// 数据源不可达时回退到旧缓存
    #[serde(default)]
    pub stale: bool,
}
