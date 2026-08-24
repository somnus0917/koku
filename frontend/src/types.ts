export type AccountType = "cash" | "credit" | "savings" | "stock";
export type CategoryKind = "expense" | "income";
export type TransactionKind = "expense" | "income" | "transfer" | "loan" | "adjustment" | "trade" | "deposit";
export type LoanType = "lend" | "borrow";
export type RecurrenceFrequency = "monthly" | "weekly";

export interface Account {
  id: number;
  name: string;
  account_type: AccountType;
  currency: string;
  balance: string;
  /** 信用额度，仅信用账户有值 */
  credit_limit: string | null;
  /** 账单日（1~31），仅信用账户有值 */
  statement_day: number | null;
  /** 还款日（1~31），仅信用账户有值 */
  due_day: number | null;
}

/** 信用卡账单摘要（已出账周期使用不可变快照）。 */
export interface CreditCardSummary {
  account_id: number;
  currency: string;
  credit_limit: string | null;
  used_credit: string;
  available_credit: string | null;
  statement_day: number | null;
  due_day: number | null;
  current_statement_amount: string | null;
  unbilled_amount: string | null;
  next_statement_date: string | null;
  next_due_date: string | null;
}

export interface CreditCardStatement {
  statement_date: string;
  due_at: string | null;
  amount: string;
  outstanding: string;
}

export interface Category {
  id: number;
  name: string;
  kind: CategoryKind;
  /** 用户自选图标（lucide 图标名）；无则前端按名称回退默认视觉 */
  icon: string | null;
}

export type UserRole = "admin" | "member";

export interface AuthSession {
  id: number;
  email: string;
  role: UserRole;
  /** 该账号是否已启用二步验证（TOTP）。 */
  totp_enabled: boolean;
}

/** 登录第一步返回的二步验证挑战（无会话 Cookie，需用 totp_token + 动态码完成登录）。 */
export interface TotpChallenge {
  totp_required: true;
  totp_token: string;
  email: string;
}

/** 账本用户（管理员可见；password_hash 不会下发）。 */
export interface User {
  id: number;
  email: string;
  role: UserRole;
  enabled: boolean;
  /** 该账号是否已启用二步验证（TOTP）。 */
  totp_enabled: boolean;
  created_at: string;
}

export interface Transaction {
  id: number;
  kind: TransactionKind;
  account_id: number;
  to_account_id: number | null;
  category_id: number | null;
  amount: string;
  currency: string;
  settled_amount: string;
  target_amount: string | null;
  target_currency: string | null;
  occurred_at: string;
  note: string;
  voided_at: string | null;
  loan_id: number | null;
  reimbursable_at: string | null;
  reimbursed_at: string | null;
  reimbursed_amount: string;
  refunded_amount: string;
  /** 退款收入关联的原支出；票根墙将其折叠到原票根。 */
  refund_expense_id?: number | null;
  has_receipt: boolean;
  /** 是否已有拆分分类（父交易仅作默认分类，统计以拆分为准）。 */
  has_splits: boolean;
  tags: string[];
  payee_id: number | null;
  payee_name: string | null;
  raw_description: string | null;
}

/** 商户/收款方：从流水描述中识别或用户手动设置，参与自动分类学习。 */
export interface Payee {
  id: number;
  name: string;
  created_at: string;
}

export interface Loan {
  id: number;
  loan_type: LoanType;
  counterparty: string;
  currency: string;
  principal: string;
  outstanding: string;
  account_id: number;
  opened_at: string;
  note: string;
  closed_at: string | null;
  due_at: string | null;
}

export interface Deposit {
  id: number;
  source_account_id: number;
  amount: string;
  currency: string;
  rate: string;
  term_days: number;
  opened_at: string;
  maturity_at: string;
  settled_at: string | null;
  note: string;
}

export interface DepositSettlement {
  interest: string;
  transfer: Transaction;
}

export interface CategoryExpense {
  category_id: number;
  category_name: string;
  amount: string;
  percentage: string;
  /** 该分类当月预算上限；未设置时服务端不返回该字段 */
  budget_limit?: string | null;
}

export interface Budget {
  category_id: number;
  category_name: string;
  category_kind: CategoryKind;
  year: number;
  month: number;
  limit_amount: string;
}

export interface RecurringRule {
  id: number;
  kind: TransactionKind;
  account_id: number;
  category_id: number;
  amount: string;
  note: string;
  frequency: RecurrenceFrequency;
  next_due_at: string;
  paused_at: string | null;
}
export interface RecurringOccurrence { due_at: string; }

export interface Receipt {
  transaction_id: number;
  content_type: string;
  byte_length: number;
  created_at: string;
}

export interface Tag {
  id: number;
  name: string;
}

export interface Holding {
  id: number;
  account_id: number;
  symbol: string;
  shares: string;
  cost_basis: string;
  last_price: string | null;
  market: "us" | "hk" | "cn_sh" | "cn_sz" | "cn_star" | "unknown";
  price_source: "stooq" | "nasdaq" | "yahoo_finance" | "manual" | "trade" | null;
  price_as_of: string | null;
  average_cost: string;
  market_value: string | null;
  unrealized_gain: string | null;
  unrealized_return_percent: string | null;
  /** 最近一次市价刷新时间（RFC3339）；从未刷新过为 null。 */
  updated_at: string | null;
}

export interface TransactionRule {
  id: number;
  name: string;
  enabled: boolean;
  priority: number;
  description_contains: string | null;
  account_id: number | null;
  kind: "expense" | "income" | null;
  min_amount: string | null;
  max_amount: string | null;
  category_id: number | null;
  payee_name: string | null;
  tag_names: string[];
  created_at: string;
  updated_at: string;
}

/** 一条分类规则对历史流水的待确认修改，不会在预览阶段写入账本。 */
export interface TransactionRulePreview {
  transaction_id: number;
  occurred_at: string;
  note: string;
  amount: string;
  currency: string;
  current_category_id: number | null;
  suggested_category_id: number | null;
  current_payee_name: string | null;
  suggested_payee_name: string | null;
  current_tags: string[];
  suggested_tags: string[];
}

export interface ActivityEvent {
  id: number;
  action: string;
  entity_type: string;
  entity_id: number;
  summary: string;
  occurred_at: string;
}

export interface ImportProfile {
  id: number;
  name: string;
  format: "auto" | "csv" | "qif" | "ofx";
  account_id: number | null;
  category_id: number | null;
  currency: string | null;
  created_at: string;
  updated_at: string;
}

export interface Bill {
  id: number;
  name: string;
  account_id: number;
  category_id: number;
  amount: string;
  due_day: number;
  active: boolean;
  note: string;
  created_at: string;
  updated_at: string;
}

export interface SavingsGoal {
  id: number;
  name: string;
  account_id: number | null;
  target_amount: string;
  current_amount: string;
  target_date: string | null;
  created_at: string;
  updated_at: string;
}

export interface MonthlySummary {
  year: number;
  month: number;
  currency: string;
  total_income: string;
  total_expense: string;
  net: string;
  expenses_by_category: CategoryExpense[];
}

export interface MonthlyTrendPoint {
  year: number;
  month: number;
  total_income: string;
  total_expense: string;
  net: string;
}

export interface CashFlowItem {
  category_id: number;
  category_name: string;
  amount: string;
  percentage: string;
}

export interface CashFlowSummary {
  year: number;
  month: number;
  currency: string;
  total_income: string;
  total_expense: string;
  retained: string;
  flow_total: string;
  income_sources: CashFlowItem[];
  expense_destinations: CashFlowItem[];
}

/** 标签汇总：同时带有全部指定标签的收支流水；year/month 为 null 表示全部历史。 */
export interface TagSummary {
  tags: string[];
  year: number | null;
  month: number | null;
  currency: string;
  total_income: string;
  total_expense: string;
  retained: string;
  income_sources: CashFlowItem[];
  expense_destinations: CashFlowItem[];
}

export interface BalanceSummary {
  currency: string;
  total_assets: string;
  total_liabilities: string;
  net_worth: string;
}

export interface RateQuote {
  from: string;
  to: string;
  /** 参考汇率：1 from = rate to */
  rate: string;
  /** 汇率生效日期（YYYY-MM-DD） */
  date: string;
  source: string;
  /** 数据源不可达时回退到旧缓存 */
  stale?: boolean;
}

export interface AppData {
  accounts: Account[];
  categories: Category[];
  transactions: Transaction[];
  monthly: MonthlySummary;
  cashFlow: CashFlowSummary;
  balance: BalanceSummary;
  loans: Loan[];
  budgets: Budget[];
  recurring: RecurringRule[];
  tags: Tag[];
  holdings: Holding[];
  deposits: Deposit[];
}

/** 年度汇总：某自然年逐月收支 + 全年合计 + 按分类的收入/支出明细。 */
export interface YearlySummary {
  year: number;
  currency: string;
  total_income: string;
  total_expense: string;
  net: string;
  /** 1 月在前、12 个自然月的逐月收支；无流水的月份补零。 */
  months: MonthlyTrendPoint[];
  income_sources: CashFlowItem[];
  expense_destinations: CashFlowItem[];
}

/** 滚动平均序列中的一个月点：当月收支 + 截至该月的 trailing window 平均值。 */
export interface RollingPoint {
  year: number;
  month: number;
  income: string;
  expense: string;
  net: string;
  income_avg: string;
  expense_avg: string;
  net_avg: string;
}

/** 滚动平均：最近 `months` 个月的收支趋势，逐月给出 trailing window 均值。 */
export interface RollingSummary {
  currency: string;
  months: number;
  /** 平均窗口（月）。 */
  window: number;
  points: RollingPoint[];
}

/** 备份元信息：zip 包内相对路径（如 `koku.db`、`ledgers/ledger-1.db`）。 */
export interface BackupMeta {
  id: string;
  filename: string;
  /** RFC3339 创建时间（UTC）。 */
  created_at: string;
  size_bytes: number;
  files: string[];
  /** 已上传 R2 的对象键（如 `koku/koku-20260101-120000.zip`）；未上传为 null。 */
  r2_key?: string | null;
}

/** 管理员可见的 R2 异地备份状态。 */
export interface R2Status {
  enabled: boolean;
  bucket?: string;
  prefix?: string;
  endpoint?: string;
  last_uploaded?: { backup_id: string; size_bytes: number } | null;
}

/** 导入时某行被跳过/失败的原因。 */
export interface ImportIssue {
  line: number;
  message: string;
}

/** 导入时的中置信度分类建议：对应一笔已导入交易，等待用户人工确认。 */
export interface CategorySuggestion {
  transaction_id: number;
  payee_id: number;
  payee_name: string;
  current_category_id: number;
  current_category_name: string;
  suggested_category_id: number;
  suggested_category_name: string;
  /** 置信度（0..=1 的小数，如 0.83）。 */
  confidence: string;
}

/** 一次批量导入的统计结果。 */
export interface ImportResult {
  batch_id: string;
  format: string;
  account_id: number;
  imported: number;
  skipped_duplicates: number;
  failed: number;
  issues: ImportIssue[];
  /** 成功导入且识别出商户（Payee）的条数。 */
  payees_recognized: number;
  /** 成功导入且高置信度自动应用分类的条数。 */
  categories_auto_applied: number;
  /** 成功导入且产生中等置信度分类建议（未自动应用）的条数。 */
  category_suggestion_count: number;
  /** 中等置信度分类建议明细（每条对应一笔已导入交易）。 */
  category_suggestions: CategorySuggestion[];
  /** 成功导入但未能识别商户的条数。 */
  unrecognized: number;
}

export interface ImportPreview {
  format: string;
  total_rows: number;
  income_rows: number;
  expense_rows: number;
  issues: ImportIssue[];
  sample_rows: Array<{ line: number; date: string; amount: string; note: string; currency: string | null }>;
}

/** 交易拆分：把一笔 expense/income 的金额按多个分类归属（余额只动一次）。 */
export interface TransactionSplit {
  id: number;
  transaction_id: number;
  category_id: number;
  amount: string;
  note: string | null;
  created_at: string;
}

export type ReconciliationStatus = "open" | "completed" | "cancelled";

/** 一次账户对账：把对账单余额与账面余额核对，差额可生成调整流水。 */
export interface Reconciliation {
  id: number;
  account_id: number;
  /** 对账日（YYYY-MM-DD）。 */
  statement_date: string;
  statement_balance: string;
  book_balance: string;
  status: ReconciliationStatus;
  opened_at: string;
  completed_at: string | null;
  /** 完成时差额 ≠ 0 会自动生成调整流水，记录其流水 id；无差额为 null。 */
  adjustment_transaction_id: number | null;
  note: string;
}

/** 到期提醒项（存款到期 / 借款到期 / 信用卡账单）。 */
export interface ReminderItem {
  kind: "deposit" | "loan" | "credit_card" | "bill";
  id: number;
  title: string;
  amount: string;
  currency: string;
  due_at: string;
  overdue: boolean;
  days_left: number;
}
