import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { login, verifyTotp } from "../../api";
import { changeLanguage } from "../../i18n";
import type { AuthSession } from "../../types";
import { LoginPage } from "./LoginPage";

vi.mock("../../api", () => ({
  ApiError: class ApiError extends Error {
    status: number;
    constructor(message: string, status: number) {
      super(message);
      this.status = status;
    }
  },
  login: vi.fn(),
  verifyTotp: vi.fn()
}));

const session: AuthSession = { id: 7, email: "owner@example.com", role: "admin", totp_enabled: false };

describe("LoginPage", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    await changeLanguage("en");
  });

  it("submits credentials and returns an authenticated session", async () => {
    vi.mocked(login).mockResolvedValue(session);
    const onAuthenticated = vi.fn();
    render(<LoginPage onAuthenticated={onAuthenticated} />);

    await userEvent.type(screen.getByRole("textbox", { name: "Email" }), "owner@example.com");
    await userEvent.type(screen.getByLabelText("Password"), "correct horse battery staple");
    await userEvent.click(screen.getByRole("button", { name: "Sign in securely" }));

    expect(login).toHaveBeenCalledWith("owner@example.com", "correct horse battery staple");
    expect(onAuthenticated).toHaveBeenCalledWith(session);
  });

  it("completes the TOTP challenge before authenticating", async () => {
    vi.mocked(login).mockResolvedValue({
      totp_required: true,
      totp_token: "challenge-token",
      email: "owner@example.com"
    });
    vi.mocked(verifyTotp).mockResolvedValue({ ...session, totp_enabled: true });
    const onAuthenticated = vi.fn();
    render(<LoginPage onAuthenticated={onAuthenticated} />);

    await userEvent.type(screen.getByRole("textbox", { name: "Email" }), "owner@example.com");
    await userEvent.type(screen.getByLabelText("Password"), "secret");
    await userEvent.click(screen.getByRole("button", { name: "Sign in securely" }));
    await userEvent.type(await screen.findByRole("textbox", { name: "Code" }), "123456");
    await userEvent.click(screen.getByRole("button", { name: "Verify and sign in" }));

    expect(verifyTotp).toHaveBeenCalledWith("challenge-token", "123456");
    expect(onAuthenticated).toHaveBeenCalledWith({ ...session, totp_enabled: true });
  });
});
