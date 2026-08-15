import type {
  Account,
  AccountType,
  AppData,
  BalanceSummary,
  CashFlowSummary,
  Category,
  CategoryKind,
  MonthlySummary,
  Transaction,
  TransactionKind
} from "./types";

const API_BASE = import.meta.env.VITE_API_BASE_URL ?? "";

interface Envelope<T> {
  data: T;
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...init?.headers
    }
  });
  const payload = (await response.json().catch(() => ({}))) as Partial<Envelope<T>> & {
    error?: string;
  };
  if (!response.ok) {
    throw new Error(payload.error ?? `请求失败（${response.status}）`);
  }
  if (payload.data === undefined) {
    throw new Error("服务返回了无效数据");
  }
  return payload.data;
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
  const [accounts, categories, transactions, monthly, cashFlow, balance] = await Promise.all([
    request<Account[]>("/api/accounts"),
    request<Category[]>("/api/categories"),
    request<Transaction[]>("/api/transactions"),
    request<MonthlySummary>(`/api/summary/monthly?${query}`),
    request<CashFlowSummary>(`/api/summary/cash-flow?${query}`),
    request<BalanceSummary>(`/api/summary/balance?${currencyQuery}`)
  ]);
  return { accounts, categories, transactions, monthly, cashFlow, balance };
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
