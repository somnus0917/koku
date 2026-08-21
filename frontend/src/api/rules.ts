import { request } from "./client";
import type { TransactionRule } from "../types";

export type TransactionRuleInput = Omit<TransactionRule, "id" | "created_at" | "updated_at">;
export const getTransactionRules = () => request<TransactionRule[]>("/api/rules");
export const createTransactionRule = (body: TransactionRuleInput) => request<TransactionRule>("/api/rules", { method: "POST", body: JSON.stringify(body) });
export const updateTransactionRule = (id: number, body: TransactionRuleInput) => request<TransactionRule>(`/api/rules/${id}`, { method: "PUT", body: JSON.stringify(body) });
export const deleteTransactionRule = (id: number) => request<void>(`/api/rules/${id}`, { method: "DELETE" });
export const applyTransactionRule = (id: number) => request<{ applied: number }>(`/api/rules/${id}/apply`, { method: "POST" });
