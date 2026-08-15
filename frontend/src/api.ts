import type {
  Account,
  AccountType,
  AppData,
  AuthSession,
  BalanceSummary,
  CashFlowSummary,
  Category,
  CategoryKind,
  DepositSettlement,
  Loan,
  LoanType,
  MonthlySummary,
  Transaction,
  TransactionKind
} from "./types";

const API_BASE = import.meta.env.VITE_API_BASE_URL ?? "";

interface Envelope<T> {
  data: T;
}

export class ApiError extends Error {
  constructor(message: string, readonly status: number) {
    super(message);
    this.name = "ApiError";
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, {
    ...init,
    credentials: "same-origin",
    headers: {
      "Content-Type": "application/json",
      ...init?.headers
    }
  });
  const payload = (await response.json().catch(() => ({}))) as Partial<Envelope<T>> & {
    error?: string;
  };
  if (!response.ok) {
    if (response.status === 401 && path !== "/api/auth/login") {
      window.dispatchEvent(new Event("koku:unauthorized"));
    }
    throw new ApiError(payload.error ?? `请求失败（${response.status}）`, response.status);
  }
  if (payload.data === undefined) {
    throw new Error("服务返回了无效数据");
  }
  return payload.data;
}

export function getAuthSession(): Promise<AuthSession> {
  return request("/api/auth/session");
}

export function login(username: string, password: string): Promise<AuthSession> {
  return request("/api/auth/login", {
    method: "POST",
    body: JSON.stringify({ username, password })
  });
}

export function logout(): Promise<{ logged_out: boolean }> {
  return request("/api/auth/logout", { method: "POST" });
}

export async function loadAppData(
  year: number,
  month: number,
  currency: string
): Promise<AppData> {
  const query = new URLSearchParams({
    year: String(year),
    month: String(month),
    currency
  });
  const currencyQuery = new URLSearchParams({ currency });
  const [accounts, categories, transactions, monthly, cashFlow, balance, loans] = await Promise.all([
    request<Account[]>("/api/accounts"),
    request<Category[]>("/api/categories"),
    request<Transaction[]>("/api/transactions"),
    request<MonthlySummary>(`/api/summary/monthly?${query}`),
    request<CashFlowSummary>(`/api/summary/cash-flow?${query}`),
    request<BalanceSummary>(`/api/summary/balance?${currencyQuery}`),
    request<Loan[]>("/api/loans")
  ]);
  return { accounts, categories, transactions, monthly, cashFlow, balance, loans };
}

export function createAccount(input: {
  name: string;
  account_type: AccountType;
  currency: string;
  opening_balance: string;
}): Promise<Account> {
  return request("/api/accounts", {
    method: "POST",
    body: JSON.stringify(input)
  });
}

export function updateAccount(
  id: number,
  input: { name?: string; account_type?: AccountType; currency?: string }
): Promise<Account> {
  return request(`/api/accounts/${id}`, {
    method: "PATCH",
    body: JSON.stringify(input)
  });
}

export function adjustBalance(
  id: number,
  input: { amount: string; note?: string }
): Promise<Transaction> {
  return request(`/api/accounts/${id}/adjust-balance`, {
    method: "POST",
    body: JSON.stringify(input)
  });
}

export function createCategory(input: {
  name: string;
  kind: CategoryKind;
}): Promise<Category> {
  return request("/api/categories", {
    method: "POST",
    body: JSON.stringify(input)
  });
}

export function deleteCategory(id: number): Promise<Category> {
  return request(`/api/categories/${id}`, { method: "DELETE" });
}

export function createTransaction(input: {
  kind: Exclude<TransactionKind, "transfer">;
  account_id: number;
  category_id: number;
  amount: string;
  currency: string;
  settled_amount?: string;
  occurred_at: string;
  note: string;
}): Promise<Transaction> {
  return request("/api/transactions", {
    method: "POST",
    body: JSON.stringify(input)
  });
}

export function createTransfer(input: {
  from_account_id: number;
  to_account_id: number;
  source_amount: string;
  target_amount: string;
  occurred_at: string;
  note: string;
}): Promise<Transaction> {
  return request("/api/transfers", {
    method: "POST",
    body: JSON.stringify(input)
  });
}

export function voidTransaction(id: number): Promise<Transaction> {
  return request(`/api/transactions/${id}`, { method: "DELETE" });
}

export function markReimbursable(id: number): Promise<Transaction> {
  return request(`/api/transactions/${id}/reimbursable`, { method: "POST" });
}

export function reimburse(input: {
  expense_id: number;
  account_id: number;
  amount: string;
  currency?: string;
  settled_amount?: string;
  note?: string;
}): Promise<Transaction> {
  return request("/api/reimbursements", {
    method: "POST",
    body: JSON.stringify(input)
  });
}

export function createDeposit(input: {
  from_account_id: number;
  amount: string;
  currency?: string;
  rate: string;
  term_days: number;
  note?: string;
}): Promise<Account> {
  return request("/api/deposits", {
    method: "POST",
    body: JSON.stringify(input)
  });
}

export function settleDeposit(
  accountId: number,
  to_account_id: number
): Promise<DepositSettlement> {
  return request(`/api/deposits/${accountId}/settle`, {
    method: "POST",
    body: JSON.stringify({ to_account_id })
  });
}

export function createLoan(input: {
  loan_type: LoanType;
  counterparty: string;
  currency?: string;
  amount: string;
  account_id: number;
  note?: string;
}): Promise<Loan> {
  return request("/api/loans", {
    method: "POST",
    body: JSON.stringify(input)
  });
}

export function repayLoan(
  loanId: number,
  input: {
    account_id: number;
    amount: string;
    currency?: string;
    settled_amount?: string;
    note?: string;
  }
): Promise<Loan> {
  return request(`/api/loans/${loanId}/repay`, {
    method: "POST",
    body: JSON.stringify(input)
  });
}
