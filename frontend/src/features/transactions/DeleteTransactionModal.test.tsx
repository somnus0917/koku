import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { changeLanguage } from "../../i18n";
import type { Transaction } from "../../types";
import { DeleteTransactionModal } from "./DeleteTransactionModal";

const transaction: Transaction = {
  id: 42,
  kind: "expense",
  account_id: 1,
  to_account_id: null,
  category_id: 2,
  amount: "14.00",
  currency: "CNY",
  settled_amount: "14.00",
  target_amount: null,
  target_currency: null,
  occurred_at: "2026-08-31T21:24:00Z",
  note: "Test expense",
  voided_at: "2026-09-01T09:39:00Z",
  loan_id: null,
  reimbursable_at: null,
  reimbursed_at: null,
  reimbursed_amount: "0",
  refunded_amount: "0",
  has_receipt: false,
  has_splits: false,
  tags: [],
  payee_id: null,
  payee_name: null,
  raw_description: null
};

describe("DeleteTransactionModal", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    await changeLanguage("en");
  });

  it("shows the transaction and runs permanent deletion", async () => {
    const onConfirm = vi.fn().mockResolvedValue(undefined);
    render(
      <DeleteTransactionModal
        transaction={transaction}
        onClose={vi.fn()}
        onConfirm={onConfirm}
      />
    );

    expect(screen.getByText("Test expense")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Delete permanently" }));

    await waitFor(() => expect(onConfirm).toHaveBeenCalledOnce());
  });
});
