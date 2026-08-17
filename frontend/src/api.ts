import type {
  Account,
  AccountType,
  AppData,
  AuthSession,
  BackupMeta,
  BalanceSummary,
  Budget,
  CashFlowSummary,
  Category,
  CategoryKind,
  Deposit,
  DepositSettlement,
  Holding,
  ImportResult,
  Loan,
  LoanType,
  MonthlySummary,
  MonthlyTrendPoint,
  RateQuote,
  Receipt,
  Reconciliation,
  RecurrenceFrequency,
  RecurringRule,
  ReminderItem,
  RollingSummary,
  Tag,
  TagSummary,
  TotpChallenge,
  Transaction,
  TransactionKind,
  User,
  UserRole,
  YearlySummary
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

/**
 * 登录第一步：用户名/密码正确时返回会话（带会话 Cookie）；若该账号已启用
 * 二步验证则返回 TotpChallenge（无会话 Cookie），需再用 verifyTotp 完成登录。
 */
export function login(username: string, password: string): Promise<AuthSession | TotpChallenge> {
  return request("/api/auth/login", {
    method: "POST",
    body: JSON.stringify({ username, password })
  });
}

/** 登录第二步：用第一步拿到的 totp_token + 验证器动态码换取会话。 */
export function verifyTotp(totpToken: string, code: string): Promise<AuthSession> {
  return request("/api/auth/totp", {
    method: "POST",
    body: JSON.stringify({ totp_token: totpToken, code })
  });
}

/** 开始设置二步验证：校验当前密码后返回 Base32 密钥与 otpauth URI。 */
export function totpSetup(password: string): Promise<{ secret: string; otpauth_uri: string }> {
  return request("/api/auth/totp/setup", {
    method: "POST",
    body: JSON.stringify({ password })
  });
}

/** 用验证器动态码确认开启二步验证。 */
export function totpEnable(code: string): Promise<{ enabled: boolean }> {
  return request("/api/auth/totp/enable", {
    method: "POST",
    body: JSON.stringify({ code })
  });
}

/** 用验证器动态码关闭二步验证。 */
export function totpDisable(code: string): Promise<{ enabled: boolean }> {
  return request("/api/auth/totp/disable", {
    method: "POST",
    body: JSON.stringify({ code })
  });
}

export function logout(): Promise<{ logged_out: boolean }> {
  return request("/api/auth/logout", { method: "POST" });
}

export function changePassword(
  oldPassword: string,
  newPassword: string
): Promise<{ changed: boolean }> {
  return request("/api/auth/password", {
    method: "POST",
    body: JSON.stringify({ old_password: oldPassword, new_password: newPassword })
  });
}

/** 管理员：用户列表。 */
export function listUsers(): Promise<User[]> {
  return request("/api/users");
}

/** 管理员：创建成员用户。 */
export function createUser(username: string, password: string): Promise<User> {
  return request("/api/users", {
    method: "POST",
    body: JSON.stringify({ username, password })
  });
}

/** 管理员：重置某用户密码（其会话全部作废）。 */
export function resetUserPassword(userId: number, password: string): Promise<{ changed: boolean }> {
  return request(`/api/users/${userId}/password`, {
    method: "POST",
    body: JSON.stringify({ password })
  });
}

/** 管理员：启用/停用某用户（停用立即作废其会话）。 */
export function setUserEnabled(userId: number, enabled: boolean): Promise<{ enabled: boolean }> {
  return request(`/api/users/${userId}/enabled`, {
    method: "POST",
    body: JSON.stringify({ enabled })
  });
}

/** 管理员：删除用户（连带其独立账本数据），不可恢复。 */
export function deleteUser(userId: number): Promise<{ deleted: boolean }> {
  return request(`/api/users/${userId}`, { method: "DELETE" });
}

/** 汇总侧数据（除交易流水外的所有首屏数据）。 */
export type SummaryData = Omit<AppData, "transactions">;

export async function loadSummaryData(
  year: number,
  month: number,
  currency: string
): Promise<SummaryData> {
  const query = new URLSearchParams({
    year: String(year),
    month: String(month),
    currency
  });
  const currencyQuery = new URLSearchParams({ currency });
  const budgetQuery = new URLSearchParams({ year: String(year), month: String(month) });
  const [accounts, categories, budgets, monthly, cashFlow, balance, loans, recurring, tags, holdings, deposits] = await Promise.all([
    request<Account[]>("/api/accounts"),
    request<Category[]>("/api/categories"),
    request<Budget[]>(`/api/budgets?${budgetQuery}`),
    request<MonthlySummary>(`/api/summary/monthly?${query}`),
    request<CashFlowSummary>(`/api/summary/cash-flow?${query}`),
    request<BalanceSummary>(`/api/summary/balance?${currencyQuery}`),
    request<Loan[]>("/api/loans"),
    request<RecurringRule[]>("/api/recurring"),
    request<Tag[]>("/api/tags"),
    request<Holding[]>("/api/holdings"),
    request<Deposit[]>("/api/deposits")
  ]);
  return { accounts, categories, budgets, monthly, cashFlow, balance, loans, recurring, tags, holdings, deposits };
}

/** 分页读取交易流水；传入 `year`/`month` 时按自然月过滤，否则读取全部。 */
export function loadTransactions(
  offset: number,
  limit: number,
  year?: number,
  month?: number
): Promise<Transaction[]> {
  const query = new URLSearchParams({
    limit: String(limit),
    offset: String(offset)
  });
  if (year !== undefined && month !== undefined) {
    query.set("year", String(year));
    query.set("month", String(month));
  }
  return request<Transaction[]>(`/api/transactions?${query.toString()}`);
}

export function buyStock(input: {
  account_id: number;
  symbol: string;
  shares: string;
  price: string;
  occurred_at?: string;
  note?: string;
}): Promise<Transaction> {
  return request("/api/holdings/buy", {
    method: "POST",
    body: JSON.stringify(input)
  });
}

export function sellStock(input: {
  account_id: number;
  symbol: string;
  shares: string;
  price: string;
  occurred_at?: string;
  note?: string;
}): Promise<Transaction> {
  return request("/api/holdings/sell", {
    method: "POST",
    body: JSON.stringify(input)
  });
}

export function setHoldingPrice(holdingId: number, price: string): Promise<Holding> {
  return request(`/api/holdings/${holdingId}/price`, {
    method: "PUT",
    body: JSON.stringify({ price })
  });
}

/** 刷新过期/缺失市价的全部持仓（只刷过期项），返回刷新统计与最新持仓。 */
export function refreshHoldings(): Promise<{
  refreshed: number;
  failed: { symbol: string; error: string }[];
  holdings: Holding[];
}> {
  return request("/api/holdings/refresh", { method: "POST" });
}

/** 强制刷新单只持仓的市价。 */
export function refreshHolding(id: number): Promise<Holding> {
  return request(`/api/holdings/${id}/refresh`, { method: "POST" });
}

/** 某账户的对账列表（按开启时间倒序）。 */
export function listReconciliations(accountId: number): Promise<Reconciliation[]> {
  const query = new URLSearchParams({ account_id: String(accountId) });
  return request(`/api/reconciliations?${query.toString()}`);
}

/** 新建对账（同一账户同时只能有一笔进行中）。 */
export function createReconciliation(input: {
  account_id: number;
  statement_date: string;
  statement_balance: string;
  note?: string;
}): Promise<Reconciliation> {
  return request("/api/reconciliations", {
    method: "POST",
    body: JSON.stringify(input)
  });
}

/** 完成对账：差额 ≠ 0 时后端自动生成调整流水。 */
export function completeReconciliation(id: number): Promise<Reconciliation> {
  return request(`/api/reconciliations/${id}/complete`, { method: "POST" });
}

/** 取消对账。 */
export function cancelReconciliation(id: number): Promise<Reconciliation> {
  return request(`/api/reconciliations/${id}/cancel`, { method: "POST" });
}

/** 给交易上传小票/发票附件（multipart；文件字段名为 `file`）。 */
export function uploadReceipt(transactionId: number, file: File): Promise<Receipt> {
  const form = new FormData();
  form.append("file", file);
  return fetch(`${API_BASE}/api/transactions/${transactionId}/receipt`, {
    method: "POST",
    credentials: "same-origin",
    body: form
  }).then(async (response) => {
    const payload = (await response.json().catch(() => ({}))) as Partial<Envelope<Receipt>> & {
      error?: string;
    };
    if (!response.ok) {
      throw new ApiError(payload.error ?? `上传失败（${response.status}）`, response.status);
    }
    if (payload.data === undefined) {
      throw new Error("服务返回了无效数据");
    }
    return payload.data;
  });
}

/** 小票附件的取图地址（同源、带会话 Cookie）。 */
export function receiptUrl(transactionId: number): string {
  return `${API_BASE}/api/transactions/${transactionId}/receipt`;
}

/** 查询最近 `months` 个月的收支趋势（收入/支出/结余逐月折算到显示币种）。 */
export function loadTrend(months: number, currency: string): Promise<MonthlyTrendPoint[]> {
  const query = new URLSearchParams({ months: String(months), currency });
  return request<MonthlyTrendPoint[]>(`/api/summary/trend?${query.toString()}`);
}

/**
 * 标签汇总：同时带有全部指定标签的收支流水合计。
 * 传 `year`/`month` 统计该自然月；不传则统计全部历史。
 */
export function loadTagSummary(
  tags: string[],
  currency: string,
  year?: number,
  month?: number
): Promise<TagSummary> {
  const query = new URLSearchParams({ tags: tags.join(","), currency });
  if (year !== undefined && month !== undefined) {
    query.set("year", String(year));
    query.set("month", String(month));
  }
  return request<TagSummary>(`/api/summary/by-tag?${query.toString()}`);
}

export function createAccount(input: {
  name: string;
  account_type: AccountType;
  currency: string;
  opening_balance: string;
  credit_limit?: string;
}): Promise<Account> {
  return request("/api/accounts", {
    method: "POST",
    body: JSON.stringify(input)
  });
}

export function updateAccount(
  id: number,
  input: { name?: string; account_type?: AccountType; currency?: string; credit_limit?: string | null }
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

export function setBudget(
  categoryId: number,
  year: number,
  month: number,
  limitAmount: string
): Promise<Budget> {
  const query = new URLSearchParams({ year: String(year), month: String(month) });
  return request(`/api/budgets/${categoryId}?${query.toString()}`, {
    method: "PUT",
    body: JSON.stringify({ limit_amount: limitAmount })
  });
}

export function clearBudget(categoryId: number, year: number, month: number): Promise<Budget> {
  const query = new URLSearchParams({ year: String(year), month: String(month) });
  return request(`/api/budgets/${categoryId}?${query.toString()}`, { method: "DELETE" });
}

/** 触发月度预算自动延续（每月只执行一次，幂等），返回带入的预算条数。 */
export function rolloverBudgets(): Promise<{ copied: number }> {
  return request("/api/budgets/rollover", { method: "POST" });
}

export function createRecurringRule(input: {
  kind: "expense" | "income";
  account_id: number;
  category_id: number;
  amount: string;
  note?: string;
  frequency: RecurrenceFrequency;
  next_due_at: string;
}): Promise<RecurringRule> {
  return request("/api/recurring", {
    method: "POST",
    body: JSON.stringify(input)
  });
}

export function deleteRecurringRule(id: number): Promise<RecurringRule> {
  return request(`/api/recurring/${id}`, { method: "DELETE" });
}

/** 触发周期交易的到期生成（请求驱动，无后台任务），返回本次生成的流水。 */
export function runRecurring(): Promise<Transaction[]> {
  return request("/api/recurring/run", { method: "POST" });
}

/** 导出交易为 CSV 并触发浏览器下载；传入 year/month 时仅导出该自然月。 */
export async function exportTransactions(year?: number, month?: number): Promise<void> {
  const query = new URLSearchParams();
  if (year !== undefined && month !== undefined) {
    query.set("year", String(year));
    query.set("month", String(month));
  }
  const suffix = query.toString() ? `?${query.toString()}` : "";
  const response = await fetch(`${API_BASE}/api/transactions/export${suffix}`, {
    credentials: "same-origin"
  });
  if (!response.ok) {
    const payload = (await response.json().catch(() => ({}))) as { error?: string };
    throw new ApiError(payload.error ?? `导出失败（${response.status}）`, response.status);
  }
  const blob = await response.blob();
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  const disposition = response.headers.get("Content-Disposition") ?? "";
  link.download = disposition.match(/filename="?([^"]+)"?/)?.[1] ?? "koku-transactions.csv";
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
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
  tag_names?: string[];
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
  return request(`/api/transactions/${id}/void`, { method: "POST" });
}

/** 撤销删除：恢复一笔已撤销的流水，重新应用其余额影响。 */
export function restoreTransaction(id: number): Promise<Transaction> {
  return request(`/api/transactions/${id}/restore`, { method: "POST" });
}

/** 永久删除一笔已撤销的流水（连带小票、标签与报销关联），不可恢复。 */
export async function deleteTransactionPermanently(id: number): Promise<void> {
  const response = await fetch(`${API_BASE}/api/transactions/${id}`, {
    method: "DELETE",
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" }
  });
  if (!response.ok) {
    if (response.status === 401) {
      window.dispatchEvent(new Event("koku:unauthorized"));
    }
    let message = `请求失败（${response.status}）`;
    try {
      const body = await response.json();
      if (body?.error) message = body.error;
    } catch {
      // 204/非 JSON 错误体直接使用默认文案。
    }
    throw new ApiError(message, response.status);
  }
}

export function updateTransaction(
  id: number,
  input: {
    note?: string;
    occurred_at?: string;
    category_id?: number;
    amount?: string;
    account_id?: number;
    settled_amount?: string;
    tag_names?: string[];
  }
): Promise<Transaction> {
  return request(`/api/transactions/${id}`, {
    method: "PATCH",
    body: JSON.stringify(input)
  });
}

export function markReimbursable(id: number): Promise<Transaction> {
  return request(`/api/transactions/${id}/reimbursable`, { method: "POST" });
}

export function unmarkReimbursable(id: number): Promise<Transaction> {
  return request(`/api/transactions/${id}/reimbursable`, { method: "DELETE" });
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
}): Promise<Deposit> {
  return request("/api/deposits", {
    method: "POST",
    body: JSON.stringify(input)
  });
}

export function settleDeposit(
  depositId: number,
  to_account_id: number
): Promise<DepositSettlement> {
  return request(`/api/deposits/${depositId}/settle`, {
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
  due_at?: string;
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

/** 汇率提示：1 from = rate to，服务端带本地缓存，数据源不可达时可能返回 stale 快照。 */
export function rateHint(from: string, to: string): Promise<RateQuote> {
  const query = new URLSearchParams({ from, to });
  return request(`/api/rates?${query.toString()}`);
}

/** 年度汇总：某自然年的逐月收支 + 全年合计 + 分类明细。 */
export function loadYearlySummary(year: number, currency: string): Promise<YearlySummary> {
  const query = new URLSearchParams({ year: String(year), currency });
  return request<YearlySummary>(`/api/summary/yearly?${query.toString()}`);
}

/** 滚动平均：最近 `months` 个月的收支趋势 + trailing window 均值。 */
export function loadRollingSummary(
  months: number,
  window: number,
  currency: string
): Promise<RollingSummary> {
  const query = new URLSearchParams({
    months: String(months),
    window: String(window),
    currency
  });
  return request<RollingSummary>(`/api/summary/rolling?${query.toString()}`);
}

/** 批量导入交易（CSV/QIF/OFX）；multipart 字段见后端契约。 */
export function importTransactions(
  file: File,
  input: { format?: string; account_id: number; category_id?: number; currency?: string }
): Promise<ImportResult> {
  const form = new FormData();
  form.append("file", file);
  form.append("account_id", String(input.account_id));
  if (input.format) form.append("format", input.format);
  if (input.category_id !== undefined) {
    form.append("category_id", String(input.category_id));
  }
  if (input.currency) form.append("currency", input.currency);
  return fetch(`${API_BASE}/api/transactions/import`, {
    method: "POST",
    credentials: "same-origin",
    body: form
  }).then(async (response) => {
    const payload = (await response.json().catch(() => ({}))) as Partial<Envelope<ImportResult>> & {
      error?: string;
    };
    if (!response.ok) {
      if (response.status === 401) {
        window.dispatchEvent(new Event("koku:unauthorized"));
      }
      throw new ApiError(payload.error ?? `导入失败（${response.status}）`, response.status);
    }
    if (payload.data === undefined) {
      throw new Error("服务返回了无效数据");
    }
    return payload.data;
  });
}

/** 到期提醒：未来 `days` 天内到期的存款/借款（默认 30 天）。 */
export function loadReminders(days = 30): Promise<ReminderItem[]> {
  const query = new URLSearchParams({ days: String(days) });
  return request(`/api/reminders?${query.toString()}`);
}

/** 管理员：立即发送到期提醒邮件；SMTP 未配置时后端返回 422。 */
export function sendReminderDigest(): Promise<{ sent: boolean; count: number }> {
  return request("/api/admin/reminders/send", { method: "POST" });
}

/** 管理员：列出全部备份（按时间倒序）。 */
export function listBackups(): Promise<BackupMeta[]> {
  return request<BackupMeta[]>("/api/admin/backups");
}

/** 管理员：立即创建一份备份。 */
export function createBackup(): Promise<BackupMeta> {
  return request<BackupMeta>("/api/admin/backup", { method: "POST" });
}

/** 管理员：从备份恢复（覆盖共享库与全部账本文件，所有会话失效）。 */
export function restoreBackup(id: string): Promise<{ restored: boolean }> {
  return request<{ restored: boolean }>(`/api/admin/backups/${id}/restore`, {
    method: "POST"
  });
}

/** 管理员：下载备份 zip 并触发浏览器保存。 */
export async function downloadBackup(id: string): Promise<void> {
  const response = await fetch(`${API_BASE}/api/admin/backups/${id}/download`, {
    credentials: "same-origin"
  });
  if (!response.ok) {
    if (response.status === 401) {
      window.dispatchEvent(new Event("koku:unauthorized"));
    }
    const payload = (await response.json().catch(() => ({}))) as { error?: string };
    throw new ApiError(payload.error ?? `下载失败（${response.status}）`, response.status);
  }
  const blob = await response.blob();
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  const disposition = response.headers.get("Content-Disposition") ?? "";
  link.download = disposition.match(/filename="?([^"]+)"?/)?.[1] ?? `koku-${id}.zip`;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}
