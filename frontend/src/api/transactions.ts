//! 交易流水 API：查询、增删改、作废/恢复、导出、导入与小票。
import { API_BASE, ApiError, request, type Envelope } from "./client";
import i18n from "../i18n";
import type { ImportPreview, ImportResult, Receipt, Transaction, TransactionKind, TransactionSplit } from "../types";

/** 分页读取交易流水；传入 `year`/`month` 时按自然月过滤，否则读取全部。 */
export function loadTransactions(
  offset: number,
  limit: number,
  year?: number,
  month?: number,
  filters?: { search?: string; kind?: string; tags?: string[] }
): Promise<Transaction[]> {
  const query = new URLSearchParams({
    limit: String(limit),
    offset: String(offset)
  });
  if (year !== undefined && month !== undefined) {
    query.set("year", String(year));
    query.set("month", String(month));
  }
  if (filters?.search?.trim()) query.set("search", filters.search.trim());
  if (filters?.kind && filters.kind !== "all") query.set("kind", filters.kind);
  if (filters?.tags?.length) query.set("tags", filters.tags.join(","));
  return request<Transaction[]>(`/api/transactions?${query.toString()}`);
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
      throw new ApiError(payload.error ?? i18n.t("api.uploadFailed", { status: response.status }), response.status);
    }
    if (payload.data === undefined) {
      throw new Error(i18n.t("api.invalidData"));
    }
    return payload.data;
  });
}
/** 小票附件的取图地址（同源、带会话 Cookie）。 */
export function receiptUrl(transactionId: number): string {
  return `${API_BASE}/api/transactions/${transactionId}/receipt`;
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
    throw new ApiError(payload.error ?? i18n.t("api.exportFailed", { status: response.status }), response.status);
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
  payee_name?: string;
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
    let message = i18n.t("api.requestFailed", { status: response.status });
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
    payee_name?: string;
    /** 拆分有变化才携带：非空数组整体替换；空数组清除（与父交易原子提交）。 */
    splits?: { category_id: number; amount: string; note?: string }[];
  }
): Promise<Transaction> {
  return request(`/api/transactions/${id}`, {
    method: "PATCH",
    body: JSON.stringify(input)
  });
}
/** 列出交易的拆分分类（无拆分为空数组）。 */
export function listTransactionSplits(transactionId: number): Promise<TransactionSplit[]> {
  return request<TransactionSplit[]>(`/api/transactions/${transactionId}/splits`);
}

/** 原子替换交易的拆分分类（金额总和须等于父交易金额）。 */
export function setTransactionSplits(
  transactionId: number,
  splits: { category_id: number; amount: string; note?: string }[]
): Promise<TransactionSplit[]> {
  return request(`/api/transactions/${transactionId}/splits`, {
    method: "PUT",
    body: JSON.stringify(splits)
  });
}

/** 清除交易的拆分分类（恢复父交易分类统计）。 */
export function clearTransactionSplits(transactionId: number): Promise<{ cleared: boolean }> {
  return request(`/api/transactions/${transactionId}/splits`, { method: "DELETE" });
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
      throw new ApiError(payload.error ?? i18n.t("api.importFailed", { status: response.status }), response.status);
    }
    if (payload.data === undefined) {
      throw new Error(i18n.t("api.invalidData"));
    }
    return payload.data;
  });
}

/** 只解析导入文件并返回前 20 条样例，不写入账本。 */
export function previewImportTransactions(
  file: File,
  input: { format?: string; account_id: number; category_id?: number; currency?: string }
): Promise<ImportPreview> {
  const form = new FormData();
  form.append("file", file);
  form.append("account_id", String(input.account_id));
  if (input.format) form.append("format", input.format);
  if (input.category_id !== undefined) form.append("category_id", String(input.category_id));
  if (input.currency) form.append("currency", input.currency);
  return fetch(`${API_BASE}/api/transactions/import/preview`, {
    method: "POST", credentials: "same-origin", body: form
  }).then(async (response) => {
    const payload = (await response.json().catch(() => ({}))) as Partial<Envelope<ImportPreview>> & { error?: string };
    if (!response.ok) throw new ApiError(payload.error ?? i18n.t("api.importFailed", { status: response.status }), response.status);
    if (payload.data === undefined) throw new Error(i18n.t("api.invalidData"));
    return payload.data;
  });
}

/** 软撤销一整批已导入流水。 */
export function undoImportBatch(batchId: string): Promise<{ undone: number }> {
  return request(`/api/transactions/import/${batchId}/undo`, { method: "POST" });
}
