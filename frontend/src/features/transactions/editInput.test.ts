import { describe, expect, it } from "vitest";
import { buildEditInput, type SplitRow } from "./editInput";
import { toLocalDateTimeValue } from "../../lib";
import type { Transaction } from "../../types";

function makeTransaction(overrides: Partial<Transaction> = {}): Transaction {
  return {
    id: 1,
    kind: "expense",
    account_id: 1,
    to_account_id: null,
    category_id: 10,
    amount: "100.00",
    currency: "CNY",
    settled_amount: "100.00",
    target_amount: null,
    target_currency: null,
    occurred_at: "2026-08-15T12:00:00Z",
    note: "旧备注",
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

function baseParams(overrides: Partial<Parameters<typeof buildEditInput>[0]> = {}) {
  const transaction = makeTransaction();
  return {
    transaction,
    note: transaction.note,
    occurredAt: toLocalDateTimeValue(transaction.occurred_at),
    categoryId: 10,
    amount: transaction.amount,
    settledAmount: transaction.settled_amount,
    accountId: transaction.account_id,
    tagNames: [],
    payeeName: "",
    foreign: false,
    splits: [] as SplitRow[],
    originalSplits: [] as SplitRow[],
    ...overrides
  };
}

describe("buildEditInput", () => {
  it("no changes produces an empty payload (modal closes without submitting)", () => {
    expect(buildEditInput(baseParams())).toEqual({});
  });

  it("splits-only change produces a single payload with just splits", () => {
    const input = buildEditInput(
      baseParams({
        splits: [
          { category_id: 10, amount: "60.00", note: "" },
          { category_id: 11, amount: "40.00", note: "" }
        ]
      })
    );
    expect(input).toEqual({
      splits: [
        { category_id: 10, amount: "60.00", note: "" },
        { category_id: 11, amount: "40.00", note: "" }
      ]
    });
  });

  it("regular field changes plus splits are submitted together in one payload", () => {
    const input = buildEditInput(
      baseParams({
        note: "新备注",
        amount: "120.00",
        splits: [
          { category_id: 10, amount: "70.00", note: "" },
          { category_id: 11, amount: "50.00", note: "" }
        ]
      })
    );
    expect(input.note).toBe("新备注");
    expect(input.amount).toBe("120.00");
    expect(input.splits).toEqual([
      { category_id: 10, amount: "70.00", note: "" },
      { category_id: 11, amount: "50.00", note: "" }
    ]);
  });

  it("clearing all splits submits an empty splits array", () => {
    const splits: SplitRow[] = [
      { category_id: 10, amount: "60.00", note: "" },
      { category_id: 11, amount: "40.00", note: "" }
    ];
    const input = buildEditInput(
      baseParams({
        transaction: makeTransaction({ id: 2 }),
        splits: [],
        originalSplits: splits
      })
    );
    expect(input.splits).toEqual([]);
  });

  it("unchanged splits do not add a splits key to the payload", () => {
    const splits: SplitRow[] = [
      { category_id: 10, amount: "60.00", note: "" },
      { category_id: 11, amount: "40.00", note: "" }
    ];
    const input = buildEditInput(
      baseParams({
        splits,
        originalSplits: splits,
        note: "只改备注"
      })
    );
    expect(input.splits).toBeUndefined();
    expect(input.note).toBe("只改备注");
  });

  it("foreign-currency amount change also submits the settled amount", () => {
    const transaction = makeTransaction({ amount: "95.00", currency: "USD", settled_amount: "680.00" });
    const input = buildEditInput(
      baseParams({
        transaction,
        amount: "100.00",
        settledAmount: "720.00",
        foreign: true
      })
    );
    expect(input.amount).toBe("100.00");
    expect(input.settled_amount).toBe("720.00");
  });

  it("payee change is trimmed before submission; blank clears it", () => {
    const input = buildEditInput(baseParams({ payeeName: "  星巴克  " }));
    expect(input.payee_name).toBe("星巴克");
    const cleared = buildEditInput(
      baseParams({
        transaction: makeTransaction({ payee_name: "星巴克" }),
        payeeName: "  "
      })
    );
    expect(cleared.payee_name).toBe("");
  });
});
