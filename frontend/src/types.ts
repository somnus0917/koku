export type AccountType = "asset" | "liability";
export type CategoryKind = "expense" | "income";
export type TransactionKind = "expense" | "income" | "transfer";

export interface Account {
  id: number;
  name: string;
  account_type: AccountType;
  currency: string;
  balance: string;
}

export interface Category {
  id: number;
  name: string;
  kind: CategoryKind;
}

export interface Transaction {
  id: number;
  kind: TransactionKind;
  account_id: number;
  to_account_id: number | null;
  category_id: number | null;
  amount: string;
  target_amount: string | null;
  occurred_at: string;
  note: string;
  voided_at: string | null;
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
  balance: BalanceSummary;
}

