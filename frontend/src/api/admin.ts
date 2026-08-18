//! 管理后台 API：用户管理与备份/R2。
import { API_BASE, ApiError, request } from "./client";
import i18n from "../i18n";
import type { BackupMeta, R2Status, User } from "../types";

/** 管理员：用户列表。 */
export function listUsers(): Promise<User[]> {
  return request("/api/users");
}
/** 管理员：创建成员用户。 */
export function createUser(username: string, password: string): Promise<User> {
  return request("/api/users", {
    method: "POST",
    body: JSON.stringify({ username, password })
  });
}
/** 管理员：重置某用户密码（其会话全部作废）。 */
export function resetUserPassword(userId: number, password: string): Promise<{ changed: boolean }> {
  return request(`/api/users/${userId}/password`, {
    method: "POST",
    body: JSON.stringify({ password })
  });
}
/** 管理员：启用/停用某用户（停用立即作废其会话）。 */
export function setUserEnabled(userId: number, enabled: boolean): Promise<{ enabled: boolean }> {
  return request(`/api/users/${userId}/enabled`, {
    method: "POST",
    body: JSON.stringify({ enabled })
  });
}
/** 管理员：删除用户（连带其独立账本数据），不可恢复。 */
export function deleteUser(userId: number): Promise<{ deleted: boolean }> {
  return request(`/api/users/${userId}`, { method: "DELETE" });
}
/** 管理员：列出全部备份（按时间倒序）。 */
export function listBackups(): Promise<BackupMeta[]> {
  return request<BackupMeta[]>("/api/admin/backups");
}
/** 管理员：立即创建一份备份。 */
export function createBackup(): Promise<BackupMeta> {
  return request<BackupMeta>("/api/admin/backup", { method: "POST" });
}
/** 管理员：从备份恢复（覆盖共享库与全部账本文件，所有会话失效）。 */
export function restoreBackup(id: string): Promise<{ restored: boolean }> {
  return request<{ restored: boolean }>(`/api/admin/backups/${id}/restore`, {
    method: "POST"
  });
}
/** 管理员：R2 异地备份状态。 */
export function r2Status(): Promise<R2Status> {
  return request("/api/admin/r2/status");
}
/** 管理员：把某个本地备份补传到 R2。 */
export function r2Upload(backupId: string): Promise<BackupMeta> {
  return request(`/api/admin/r2/upload/${backupId}`, { method: "POST" });
}
/** 管理员：删除 R2 上的某备份对象（不影响本地备份）。 */
export function r2Delete(backupId: string): Promise<{ deleted: boolean; key: string }> {
  return request(`/api/admin/r2/delete/${backupId}`, { method: "POST" });
}
/** 管理员：从 R2 下载并恢复某备份（覆盖全部数据，所有会话失效）。 */
export function r2Restore(backupId: string): Promise<{ restored: boolean; source: string }> {
  return request(`/api/admin/r2/restore/${backupId}`, { method: "POST" });
}
/** 管理员：下载备份 zip 并触发浏览器保存。 */
export async function downloadBackup(id: string): Promise<void> {
  const response = await fetch(`${API_BASE}/api/admin/backups/${id}/download`, {
    credentials: "same-origin"
  });
  if (!response.ok) {
    if (response.status === 401) {
      window.dispatchEvent(new Event("koku:unauthorized"));
    }
    const payload = (await response.json().catch(() => ({}))) as { error?: string };
    throw new ApiError(payload.error ?? i18n.t("api.downloadFailed", { status: response.status }), response.status);
  }
  const blob = await response.blob();
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  const disposition = response.headers.get("Content-Disposition") ?? "";
  link.download = disposition.match(/filename="?([^"]+)"?/)?.[1] ?? `koku-${id}.zip`;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}
