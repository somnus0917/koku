//! 周期交易 API：规则 CRUD 与到期生成。
import { request } from "./client";
import type { RecurrenceFrequency, RecurringRule, Transaction } from "../types";

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
