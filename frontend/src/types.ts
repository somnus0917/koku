export type AccountType = "cash" | "credit" | "savings" | "stock";
export type CategoryKind = "expense" | "income";
export type TransactionKind = "expense" | "income" | "transfer" | "loan";
export type LoanType = "lend" | "borrow";

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

export interface AppData {
  accounts: Account[];
  categories: Category[];
  transactions: Transaction[];
  monthly: MonthlySummary;
  cashFlow: CashFlowSummary;
  balance: BalanceSummary;
  loans: Loan[];
}
