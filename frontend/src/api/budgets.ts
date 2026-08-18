//! 预算 API：设置、清除与月度延续。
import { request } from "./client";
import type { Budget } from "../types";

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
