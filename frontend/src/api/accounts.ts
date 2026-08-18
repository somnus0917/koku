//! 账户 API：新建、更新与余额调整。
import { request } from "./client";
import type { Account, AccountType, Transaction } from "../types";

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
