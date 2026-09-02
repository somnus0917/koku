import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { changeLanguage } from "../../i18n";
import type { Account } from "../../types";
import { EditAccountModal } from "./EditAccountModal";

const account: Account = {
  id: 12,
  name: "Campus card",
  account_type: "cash",
  currency: "CNY",
  balance: "0",
  credit_limit: null,
  statement_day: null,
  due_day: null
};

describe("EditAccountModal", () => {
  beforeEach(async () => {
    vi.restoreAllMocks();
    await changeLanguage("en");
  });

  it("requires the exact account name before permanently deleting", async () => {
    const onDelete = vi.fn().mockResolvedValue(undefined);
    render(
      <EditAccountModal
        account={account}
        currencies={["CNY"]}
        onClose={vi.fn()}
        onSubmit={vi.fn()}
        onDelete={onDelete}
      />
    );

    await userEvent.click(screen.getByRole("button", { name: "Delete account" }));
    const permanentDelete = screen.getByRole("button", { name: "Permanently delete" });
    expect(permanentDelete).toBeDisabled();

    await userEvent.type(screen.getByLabelText("Type “Campus card” to confirm"), "Campus card");
    await userEvent.click(permanentDelete);

    await waitFor(() => expect(onDelete).toHaveBeenCalledOnce());
  });
});
