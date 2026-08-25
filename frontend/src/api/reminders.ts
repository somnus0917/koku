//! 到期提醒 API：查询与管理员手动发送邮件。
import { request } from "./client";
import type { ReminderItem } from "../types";

/** 到期提醒：未来 `days` 天内到期的存款/借款（默认 30 天）。 */
export function loadReminders(days = 30, currency = "CNY"): Promise<ReminderItem[]> {
  const query = new URLSearchParams({ days: String(days), currency });
  return request(`/api/reminders?${query.toString()}`);
}
/** 立即向当前登录邮箱发送本账本的到期提醒；SMTP 未配置时后端返回 422。 */
export function sendReminderDigest(currency = "CNY"): Promise<{ sent: boolean; count: number }> {
  const query = new URLSearchParams({ currency });
  return request(`/api/reminders/send?${query.toString()}`, { method: "POST" });
}
