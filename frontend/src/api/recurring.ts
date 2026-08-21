//! 周期交易 API：规则 CRUD 与到期生成。
import { request } from "./client";
import type { RecurrenceFrequency, RecurringOccurrence, RecurringRule, Transaction } from "../types";

export type RecurringInput = {
  kind: "expense" | "income";
  account_id: number;
  category_id: number;
  amount: string;
  note?: string;
  frequency: RecurrenceFrequency;
  next_due_at: string;
};

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
export function updateRecurringRule(id: number, input: RecurringInput): Promise<RecurringRule> {
  return request(`/api/recurring/${id}`, { method: "PUT", body: JSON.stringify(input) });
}
export function setRecurringPaused(id: number, paused: boolean): Promise<RecurringRule> {
  return request(`/api/recurring/${id}/paused`, { method: "POST", body: JSON.stringify({ paused }) });
}
export function getRecurringPreview(id: number): Promise<RecurringOccurrence[]> {
  return request(`/api/recurring/${id}/preview`);
}
/** 手动触发周期交易的到期生成（服务端也会定时执行），返回本次生成的流水。 */
export function runRecurring(): Promise<Transaction[]> {
  return request("/api/recurring/run", { method: "POST" });
}
