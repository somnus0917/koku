//! Payee API：搜索/列出商户（自动补全）与学习数据清理。
import { request } from "./client";
import type { Payee } from "../types";

/** 搜索/列出 Payee；`query` 为空时返回全部（按名称排序）。 */
export function listPayees(query = "", limit = 50): Promise<Payee[]> {
  const params = new URLSearchParams();
  if (query) params.set("q", query);
  params.set("limit", String(limit));
  return request<Payee[]>(`/api/payees?${params.toString()}`);
}

/** 清除自动分类学习数据（merchant_aliases 与 payee_category_stats），保留 Payee 与交易。 */
export function clearPayeeLearning(): Promise<{ cleared: boolean }> {
  return request("/api/payees/clear-learning", { method: "POST" });
}
