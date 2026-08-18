//! 定存 API：创建与到期结算。
import { request } from "./client";
import type { Deposit, DepositSettlement } from "../types";

export function createDeposit(input: {
  from_account_id: number;
  amount: string;
  currency?: string;
  rate: string;
  term_days: number;
  note?: string;
}): Promise<Deposit> {
  return request("/api/deposits", {
    method: "POST",
    body: JSON.stringify(input)
  });
}
export function settleDeposit(
  depositId: number,
  to_account_id: number
): Promise<DepositSettlement> {
  return request(`/api/deposits/${depositId}/settle`, {
    method: "POST",
    body: JSON.stringify({ to_account_id })
  });
}
