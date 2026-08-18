//! 对账 API：列表、创建、完成与取消。
import { request } from "./client";
import type { Reconciliation } from "../types";

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
