//! 报销 API：标记可报销与报销入账。
import { request } from "./client";
import type { Transaction } from "../types";

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
