//! 账户 API：新建、更新、余额调整与信用卡账单摘要。
import { request } from "./client";
import type { Account, AccountType, CreditCardStatement, CreditCardSummary, Transaction } from "../types";

export function createAccount(input: {
  name: string;
  account_type: AccountType;
  currency: string;
  opening_balance: string;
  credit_limit?: string;
  statement_day?: number;
  due_day?: number;
}): Promise<Account> {
  return request("/api/accounts", {
    method: "POST",
    body: JSON.stringify(input)
  });
}
export function updateAccount(
  id: number,
  input: {
    name?: string;
    account_type?: AccountType;
    currency?: string;
    credit_limit?: string | null;
    statement_day?: number | null;
    due_day?: number | null;
    balance_adjustment?: string;
    adjustment_note?: string;
  }
): Promise<Account> {
  return request(`/api/accounts/${id}`, {
    method: "PATCH",
    body: JSON.stringify(input)
  });
}
export function deleteAccount(id: number): Promise<Account> {
  return request(`/api/accounts/${id}`, { method: "DELETE" });
}
/** 信用卡账单摘要（额度/出账/未出账/账单与还款日）；仅对信用账户有效。 */
export function getCreditCardSummary(accountId: number): Promise<CreditCardSummary> {
  return request(`/api/accounts/${accountId}/credit-card-summary`);
}
export function getCreditCardStatements(accountId: number): Promise<CreditCardStatement[]> {
  return request(`/api/accounts/${accountId}/credit-card-statements`);
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
