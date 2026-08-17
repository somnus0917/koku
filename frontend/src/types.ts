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
}

export interface Category {
  id: number;
  name: string;
  kind: CategoryKind;
}

export type UserRole = "admin" | "member";

export interface AuthSession {
  id: number;
  username: string;
  role: UserRole;
  /** 该账号是否已启用二步验证（TOTP）。 */
  totp_enabled: boolean;
}

/** 登录第一步返回的二步验证挑战（无会话 Cookie，需用 totp_token + 动态码完成登录）。 */
export interface TotpChallenge {
  totp_required: true;
  totp_token: string;
  username: string;
}

/** 账本用户（管理员可见；password_hash 不会下发）。 */
export interface User {
  id: number;
  username: string;
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
  has_receipt: boolean;
  tags: string[];
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
  average_cost: string;
  /** 最近一次市价刷新时间（RFC3339）；从未刷新过为 null。 */
  updated_at: string | null;
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
}

/** 导入时某行被跳过/失败的原因。 */
export interface ImportIssue {
  line: number;
  message: string;
}

/** 一次批量导入的统计结果。 */
export interface ImportResult {
  format: string;
  account_id: number;
  imported: number;
  skipped_duplicates: number;
  failed: number;
  issues: ImportIssue[];
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

/** 到期提醒项（存款到期 / 借款到期）。 */
export interface ReminderItem {
  kind: "deposit" | "loan";
  id: number;
  title: string;
  amount: string;
  currency: string;
  due_at: string;
  overdue: boolean;
  days_left: number;
}
