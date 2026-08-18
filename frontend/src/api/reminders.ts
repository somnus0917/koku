//! 到期提醒 API：查询与管理员手动发送邮件。
import { request } from "./client";
import type { ReminderItem } from "../types";

/** 到期提醒：未来 `days` 天内到期的存款/借款（默认 30 天）。 */
export function loadReminders(days = 30): Promise<ReminderItem[]> {
  const query = new URLSearchParams({ days: String(days) });
  return request(`/api/reminders?${query.toString()}`);
}
/** 管理员：立即发送到期提醒邮件；SMTP 未配置时后端返回 422。 */
export function sendReminderDigest(): Promise<{ sent: boolean; count: number }> {
  return request("/api/admin/reminders/send", { method: "POST" });
}
