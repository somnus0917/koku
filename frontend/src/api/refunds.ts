//! 退款 API：为原支出创建关联收入，并入账到指定账户。
import { request } from "./client";
import type { Transaction } from "../types";

export function refund(input: {
  expense_id: number;
  account_id: number;
  amount: string;
  currency?: string;
  settled_amount?: string;
  note?: string;
}): Promise<Transaction> {
  return request("/api/refunds", {
    method: "POST",
    body: JSON.stringify(input)
  });
}
