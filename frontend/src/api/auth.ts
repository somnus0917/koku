//! 认证 API：会话、登录、TOTP 与密码修改。
import { request } from "./client";
import type { AuthSession, TotpChallenge } from "../types";

export function getAuthSession(): Promise<AuthSession> {
  return request("/api/auth/session");
}
/**
 * 登录第一步：用户名/密码正确时返回会话（带会话 Cookie）；若该账号已启用
 * 二步验证则返回 TotpChallenge（无会话 Cookie），需再用 verifyTotp 完成登录。
 */
export function login(username: string, password: string): Promise<AuthSession | TotpChallenge> {
  return request("/api/auth/login", {
    method: "POST",
    body: JSON.stringify({ username, password })
  });
}
/** 登录第二步：用第一步拿到的 totp_token + 验证器动态码换取会话。 */
export function verifyTotp(totpToken: string, code: string): Promise<AuthSession> {
  return request("/api/auth/totp", {
    method: "POST",
    body: JSON.stringify({ totp_token: totpToken, code })
  });
}
/** 开始设置二步验证：校验当前密码后返回 Base32 密钥与 otpauth URI。 */
export function totpSetup(password: string): Promise<{ secret: string; otpauth_uri: string }> {
  return request("/api/auth/totp/setup", {
    method: "POST",
    body: JSON.stringify({ password })
  });
}
/** 用验证器动态码确认开启二步验证。 */
export function totpEnable(code: string): Promise<{ enabled: boolean }> {
  return request("/api/auth/totp/enable", {
    method: "POST",
    body: JSON.stringify({ code })
  });
}
/** 用验证器动态码关闭二步验证。 */
export function totpDisable(code: string): Promise<{ enabled: boolean }> {
  return request("/api/auth/totp/disable", {
    method: "POST",
    body: JSON.stringify({ code })
  });
}
export function logout(): Promise<{ logged_out: boolean }> {
  return request("/api/auth/logout", { method: "POST" });
}
export function changePassword(
  oldPassword: string,
  newPassword: string
): Promise<{ changed: boolean }> {
  return request("/api/auth/password", {
    method: "POST",
    body: JSON.stringify({ old_password: oldPassword, new_password: newPassword })
  });
}
