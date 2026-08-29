import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { changeLanguage } from "../i18n";
import { MobileBottomNav } from "./MobileBottomNav";

describe("MobileBottomNav", () => {
  beforeEach(async () => {
    await changeLanguage("en");
  });

  it("shows the four primary destinations and dispatches navigation", async () => {
    const onNavigate = vi.fn();
    render(<MobileBottomNav activeView="dashboard" onNavigate={onNavigate} onQuickAdd={vi.fn()} />);

    expect(screen.getByRole("navigation", { name: "Mobile navigation" })).toBeInTheDocument();
    expect(screen.getAllByRole("button")).toHaveLength(5);
    await userEvent.click(screen.getByRole("button", { name: "Accounts" }));

    expect(onNavigate).toHaveBeenCalledWith("accounts");
  });

  it("exposes quick entry as an accessible action", async () => {
    const onQuickAdd = vi.fn();
    render(<MobileBottomNav activeView="transactions" onNavigate={vi.fn()} onQuickAdd={onQuickAdd} />);

    await userEvent.click(screen.getByRole("button", { name: "Add entry" }));

    expect(onQuickAdd).toHaveBeenCalledOnce();
  });
});
