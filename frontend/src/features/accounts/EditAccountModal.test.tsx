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

  it("requires confirmation before deleting the account", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(true);
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

    expect(window.confirm).toHaveBeenCalledWith(expect.stringContaining("Campus card"));
    await waitFor(() => expect(onDelete).toHaveBeenCalledOnce());
  });
});
