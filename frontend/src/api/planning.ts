import { request } from "./client";
import type { Bill, ImportProfile, SavingsGoal } from "../types";

export type ImportProfileInput = Omit<ImportProfile, "id" | "created_at" | "updated_at">;
export type BillInput = Omit<Bill, "id" | "created_at" | "updated_at">;
export type SavingsGoalInput = Omit<SavingsGoal, "id" | "created_at" | "updated_at">;

export const getImportProfiles = () => request<ImportProfile[]>("/api/import-profiles");
export const createImportProfile = (body: ImportProfileInput) => request<ImportProfile>("/api/import-profiles", { method: "POST", body: JSON.stringify(body) });
export const updateImportProfile = (id: number, body: ImportProfileInput) => request<ImportProfile>(`/api/import-profiles/${id}`, { method: "PUT", body: JSON.stringify(body) });
export const deleteImportProfile = (id: number) => request<void>(`/api/import-profiles/${id}`, { method: "DELETE" });

export const getBills = () => request<Bill[]>("/api/bills");
export const createBill = (body: BillInput) => request<Bill>("/api/bills", { method: "POST", body: JSON.stringify(body) });
export const updateBill = (id: number, body: BillInput) => request<Bill>(`/api/bills/${id}`, { method: "PUT", body: JSON.stringify(body) });
export const deleteBill = (id: number) => request<void>(`/api/bills/${id}`, { method: "DELETE" });

export const getSavingsGoals = () => request<SavingsGoal[]>("/api/savings-goals");
export const createSavingsGoal = (body: SavingsGoalInput) => request<SavingsGoal>("/api/savings-goals", { method: "POST", body: JSON.stringify(body) });
export const updateSavingsGoal = (id: number, body: SavingsGoalInput) => request<SavingsGoal>(`/api/savings-goals/${id}`, { method: "PUT", body: JSON.stringify(body) });
export const deleteSavingsGoal = (id: number) => request<void>(`/api/savings-goals/${id}`, { method: "DELETE" });
