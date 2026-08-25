import { describe, expect, it } from "vitest";
import {
  buildTransactionTrendPoints,
  formatTransactionDay,
  groupTransactionsByDay
} from "../../lib";
import type { Transaction } from "../../types";

function transaction(
  id: number,
  occurredAt: string,
  overrides: Partial<Transaction> = {}
): Transaction {
  return {
    id,
    kind: "expense",
    account_id: 1,
    to_account_id: null,
    category_id: 1,
    amount: "10",
    currency: "CNY",
    settled_amount: "10",
    target_amount: null,
    target_currency: null,
    occurred_at: occurredAt,
    note: "",
    voided_at: null,
    loan_id: null,
    reimbursable_at: null,
    reimbursed_at: null,
    reimbursed_amount: "0",
    refunded_amount: "0",
    refund_expense_id: null,
    has_receipt: false,
    has_splits: false,
    tags: [],
    payee_id: null,
    payee_name: null,
    raw_description: null,
    ...overrides
  };
}

describe("transaction timeline grouping", () => {
  it("groups transactions by calendar day while preserving first-seen order", () => {
    const first = transaction(1, "2026-08-25T01:00:00Z");
    const second = transaction(2, "2026-08-24T23:00:00Z");
    const third = transaction(3, "2026-08-25T18:00:00Z");

    expect(groupTransactionsByDay([first, second, third])).toEqual([
      { day: "2026-08-25", items: [first, third] },
      { day: "2026-08-24", items: [second] }
    ]);
  });

  it("formats a date-only value without a UTC day rollback", () => {
    expect(formatTransactionDay("2026-08-25", "en-US")).toBe("Tue, August 25");
  });
});

describe("transaction currency conversion", () => {
  it("skips a transaction when its conversion rate is missing", () => {
    const points = buildTransactionTrendPoints([
      transaction(1, "2026-08-01T10:00:00Z", { amount: "5", currency: "USD" })
    ], "CNY");

    expect(points).toEqual(Array(12).fill(0));
  });

  it("sums same-bucket transactions across currencies before accumulating", () => {
    const points = buildTransactionTrendPoints([
      transaction(1, "2026-08-01T10:00:00Z", { kind: "income", amount: "100", currency: "CNY" }),
      transaction(2, "2026-08-02T10:00:00Z", { kind: "expense", amount: "10", currency: "USD" })
    ], "CNY", { USD: 7 });

    expect(points[0]).toBe(30);
    expect(points.at(-1)).toBe(30);
  });
});
