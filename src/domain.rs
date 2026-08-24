//! 领域类型：账户/分类/交易枚举、内置分类与对外 DTO。

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::{KokuError, Result};

/// 用户角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    /// 管理员：可创建/停用/删除用户、重置密码。
    Admin,
    /// 普通成员：只能使用自己的账本。
    Member,
}

impl UserRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }

    pub fn from_db(value: &str) -> Result<Self> {
        match value {
            "admin" => Ok(Self::Admin),
            "member" => Ok(Self::Member),
            other => Err(KokuError::InvalidInput(format!(
                "unknown user role in database: {other}"
            ))),
        }
    }
}

/// 一个账本用户（每个用户拥有完全独立的账本数据）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    #[serde(rename = "email")]
    pub username: String,
    /// bcrypt 密码哈希；序列化时隐藏。
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub role: UserRole,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    /// 是否已启用 TOTP 二步验证。
    pub totp_enabled: bool,
}

/// 登录会话返回给前端的用户信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthSession {
    pub id: i64,
    #[serde(rename = "email")]
    pub username: String,
    pub role: UserRole,
    /// 当前用户是否启用 TOTP（前端据此展示二步验证设置入口）。
    pub totp_enabled: bool,
}

/// 规范化并校验登录邮箱。数据库的历史列名仍为 `username`，但新账号只允许邮箱。
pub fn normalize_email(value: &str) -> Result<String> {
    let email = value.trim().to_ascii_lowercase();
    let valid = email.len() <= 254
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && local.len() <= 64
                && !domain.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !email.chars().any(char::is_whitespace)
        });
    if !valid {
        return Err(KokuError::InvalidInput(
            "a valid email address is required".to_owned(),
        ));
    }
    Ok(email)
}

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
    /// 账单日（1~31，可空）；仅信用账户有意义
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_day: Option<u32>,
    /// 还款日（1~31，可空）；仅信用账户有意义
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_day: Option<u32>,
}

/// 信用卡账单摘要：已出账周期使用不可变快照，未出账部分按交易动态计算。
///
/// 口径说明见 [`crate::service::credit_cards`]；金额均为账户结算币种。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreditCardSummary {
    pub account_id: i64,
    /// 账户结算币种（所有金额的币种）。
    pub currency: String,
    pub credit_limit: Option<Decimal>,
    /// 已占用额度：未撤销消费 − 还款（FIFO/总负债口径，下限 0）。
    pub used_credit: Decimal,
    /// 可用额度：credit_limit − used_credit；未设额度为 `None`。
    pub available_credit: Option<Decimal>,
    pub statement_day: Option<u32>,
    pub due_day: Option<u32>,
    /// 最近一期已出账、尚未还清的消费金额；未设账单日为 `None`。
    pub current_statement_amount: Option<Decimal>,
    /// 最近账单日之后、尚未出账的消费金额；未设账单日为 `None`。
    pub unbilled_amount: Option<Decimal>,
    /// 下一次账单日（YYYY-MM-DD）；未设账单日为 `None`。
    pub next_statement_date: Option<NaiveDate>,
    /// 下一次还款日（YYYY-MM-DD）；未设还款日为 `None`。
    pub next_due_date: Option<NaiveDate>,
}

/// 一期已出账信用卡账单快照及按 FIFO 口径估算的未还金额。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreditCardStatement {
    pub statement_date: NaiveDate,
    pub due_at: Option<DateTime<Utc>>,
    pub amount: Decimal,
    pub outstanding: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub kind: CategoryKind,
    /// 用户自选图标（lucide 图标名）；NULL 时前端按名称回退到默认视觉。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
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
    /// 累计已退款金额（原币种）
    pub refunded_amount: Decimal,
    /// 是否已挂有小票/发票附件
    pub has_receipt: bool,
    /// 是否已有拆分分类（父交易仅作默认分类，统计以拆分为准）
    #[serde(default)]
    pub has_splits: bool,
    /// 关联标签（标签名，按创建顺序）
    #[serde(default)]
    pub tags: Vec<String>,
    /// 关联的商户/收款方；导入自动识别或用户手动设置
    pub payee_id: Option<i64>,
    /// 商户名称（由 payee_id 关联出的展示字段）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payee_name: Option<String>,
    /// 导入时的原始流水描述（机器识别用，与用户备注区分）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_description: Option<String>,
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

/// 商户/收款方：从流水描述中识别出的业务实体，属于单个用户账本。
/// 每个 Payee 可关联多条 `merchant_aliases`（原始描述 → Payee）与
/// 历史分类统计（`payee_category_stats`），用于自动分类学习。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payee {
    pub id: i64,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

/// 某 Payee 的历史分类预测结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryPrediction {
    pub category_id: i64,
    pub category_name: String,
    /// 置信度（0..=1 的小数，如 0.974）。
    pub confidence: Decimal,
    /// 是否达到自动应用阈值（否则为「建议」）。
    pub auto_applied: bool,
}

/// 导入时的中置信度分类建议：对应一笔已导入交易，等待用户人工确认。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategorySuggestion {
    pub transaction_id: i64,
    pub payee_id: i64,
    pub payee_name: String,
    /// 该交易当前真实分类（行内分类或默认分类）。
    pub current_category_id: i64,
    pub current_category_name: String,
    /// 建议采纳的分类。
    pub suggested_category_id: i64,
    pub suggested_category_name: String,
    /// 置信度（0..=1 的小数，如 0.83）。
    pub confidence: Decimal,
}

/// 一只持仓：由买入时的资金账户归属，记录股数、总成本、可选市价与摊薄成本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holding {
    pub id: i64,
    pub account_id: i64,
    pub symbol: String,
    pub shares: Decimal,
    pub cost_basis: Decimal,
    pub last_price: Option<Decimal>,
    /// 代码所属市场，由后端按代码/交易所后缀识别。
    pub market: String,
    /// 最近市价的来源（Stooq、Yahoo Finance、手动或交易价）。
    pub price_source: Option<String>,
    /// 行情源所给的价格日期；手动价格使用设置当日。
    pub price_as_of: Option<String>,
    pub average_cost: Decimal,
    /// 按最近市价计算的持仓市值；未取得市价时为空，避免把成本误报为市值。
    pub market_value: Option<Decimal>,
    /// 未实现盈亏及收益率只在有最新市价时计算。
    pub unrealized_gain: Option<Decimal>,
    pub unrealized_return_percent: Option<Decimal>,
    /// 市价最近更新时间（手动设置或行情拉取）；从未设置过为 None。
    pub updated_at: Option<DateTime<Utc>>,
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

/// 周期规则未来一次实际落账的预览。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecurringOccurrence {
    pub due_at: DateTime<Utc>,
}

/// 可解释的交易自动规则：按顺序匹配，命中后可设置分类、商户与标签。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionRule {
    pub id: i64,
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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// 单条规则在某笔历史交易上的可确认修改预览。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionRulePreview {
    pub transaction_id: i64,
    pub occurred_at: DateTime<Utc>,
    pub note: String,
    pub amount: Decimal,
    pub currency: String,
    pub current_category_id: Option<i64>,
    pub suggested_category_id: Option<i64>,
    pub current_payee_name: Option<String>,
    pub suggested_payee_name: Option<String>,
    pub current_tags: Vec<String>,
    pub suggested_tags: Vec<String>,
}

/// 账本内用户可见的操作轨迹。事件只存在所属用户的独立账本中。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub id: i64,
    pub action: String,
    pub entity_type: String,
    pub entity_id: i64,
    pub summary: String,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportProfile {
    pub id: i64,
    pub name: String,
    pub format: String,
    pub account_id: Option<i64>,
    pub category_id: Option<i64>,
    pub currency: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bill {
    pub id: i64,
    pub name: String,
    pub account_id: i64,
    pub category_id: i64,
    pub amount: Decimal,
    pub due_day: u32,
    pub active: bool,
    pub note: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavingsGoal {
    pub id: i64,
    pub name: String,
    pub account_id: Option<i64>,
    pub target_amount: Decimal,
    pub current_amount: Decimal,
    pub target_date: Option<NaiveDate>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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

/// 年度汇总：某自然年逐月收支 + 全年合计 + 按分类的收入/支出明细。
/// 所有币种统一折算到显示币种；无流水的月份补零。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct YearlySummary {
    pub year: i32,
    pub currency: String,
    pub total_income: Decimal,
    pub total_expense: Decimal,
    pub net: Decimal,
    /// 1 月在前、12 个自然月的逐月收支。
    pub months: Vec<MonthlyTrendPoint>,
    pub income_sources: Vec<CashFlowItem>,
    pub expense_destinations: Vec<CashFlowItem>,
}

/// 滚动平均序列中的一个月点：当月收支 + 截至该月的 trailing window 平均值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollingPoint {
    pub year: i32,
    pub month: u32,
    pub income: Decimal,
    pub expense: Decimal,
    pub net: Decimal,
    /// 截至该月（含）最近 `window` 个月的收入平均值。
    pub income_avg: Decimal,
    pub expense_avg: Decimal,
    pub net_avg: Decimal,
}

/// 滚动平均：最近 `months` 个月的收支趋势，逐月给出 trailing window 均值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollingSummary {
    pub currency: String,
    pub months: u32,
    /// 平均窗口（月）。
    pub window: u32,
    pub points: Vec<RollingPoint>,
}

/// 交易拆分：把一笔 expense/income 的金额按多个分类归属。
/// 父交易负责真实资金流（余额只动一次）；分类统计按拆分展开。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionSplit {
    pub id: i64,
    pub transaction_id: i64,
    pub category_id: i64,
    /// 拆分行金额（> 0；所有行总和 == 父交易金额）。
    pub amount: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// 设置拆分时的单行输入（不包含 id/created_at）。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SplitInput {
    pub category_id: i64,
    pub amount: Decimal,
    #[serde(default)]
    pub note: Option<String>,
}

/// 账户对账状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconciliationStatus {
    /// 进行中：已创建，尚未完成/取消。
    Open,
    /// 已完成：必要时生成了对账调整流水。
    Completed,
    /// 已取消：不产生任何调整。
    Cancelled,
}

impl ReconciliationStatus {
    pub fn from_db(value: &str) -> Result<Self> {
        match value {
            "open" => Ok(Self::Open),
            "completed" => Ok(Self::Completed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(KokuError::InvalidInput(format!(
                "unknown reconciliation status in database: {other}"
            ))),
        }
    }
}

/// 账户对账：以对账单余额为目标，核对账面余额并在完成时自动生成调整流水。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reconciliation {
    pub id: i64,
    pub account_id: i64,
    /// 对账单日期（YYYY-MM-DD）。
    pub statement_date: String,
    /// 对账单目标余额。
    pub statement_balance: Decimal,
    /// 开始对账时的账面余额快照（供参考；调整以完成时的实际余额为准）。
    pub book_balance: Decimal,
    pub status: ReconciliationStatus,
    pub opened_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    /// 完成时自动生成的对账调整流水（差额为零时为 None）。
    pub adjustment_transaction_id: Option<i64>,
    #[serde(default)]
    pub note: String,
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
