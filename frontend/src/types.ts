export type AccountType = "cash" | "credit" | "savings" | "stock";
export type CategoryKind = "expense" | "income";
export type TransactionKind = "expense" | "income" | "transfer" | "loan" | "adjustment";
export type LoanType = "lend" | "borrow";
export type RecurrenceFrequency = "monthly" | "weekly";

export interface Account {
  id: number;
  name: string;
  account_type: AccountType;
  currency: string;
  balance: string;
  /** 定期利率（百分比），仅定期存款账户有值 */
  interest_rate: string | null;
  /** 定期到期日，仅定期存款账户有值 */
  maturity_at: string | null;
  /** 信用额度，仅信用账户有值 */
  credit_limit: string | null;
}

export interface Category {
  id: number;
  name: string;
  kind: CategoryKind;
}

export interface AuthSession {
  username: string;
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
}
