//! 管理员系统页：备份/恢复（仅 admin 角色可见，非 admin 请求会被后端 403）。

import { useEffect, useRef, useState } from "react";
import {
  DatabaseBackup,
  Download,
  LoaderCircle,
  Plus,
  RotateCcw,
  ShieldAlert
} from "lucide-react";
import {
  createBackup,
  downloadBackup,
  listBackups,
  restoreBackup
} from "../api";
import { formatDate } from "../lib";
import { EmptyState, PageTitle } from "./ledger";
import type { BackupMeta } from "../types";

/** 人类可读的文件大小（lib.ts 暂无 formatBytes，这里本地实现）。 */
function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = "B";
  for (const next of units) {
    if (value < 1024) break;
    value /= 1024;
    unit = next;
  }
  return `${value >= 100 ? value.toFixed(0) : value.toFixed(1)} ${unit}`;
}

export function SystemAdminPage() {
  const [backups, setBackups] = useState<BackupMeta[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const successTimer = useRef<number | undefined>(undefined);
  const [creating, setCreating] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);

  const flash = (message: string) => {
    setSuccess(message);
    window.clearTimeout(successTimer.current);
    successTimer.current = window.setTimeout(() => setSuccess(null), 3200);
  };

  const refresh = async () => {
    try {
      setBackups(await listBackups());
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "加载备份列表失败");
    }
  };
  useEffect(() => {
    void refresh();
  }, []);

  const create = async () => {
    setCreating(true);
    setError(null);
    try {
      await createBackup();
      flash("备份已创建");
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "创建备份失败");
    } finally {
      setCreating(false);
    }
  };

  const download = async (item: BackupMeta) => {
    setBusyId(item.id);
    setError(null);
    try {
      await downloadBackup(item.id);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "下载备份失败");
    } finally {
      setBusyId(null);
    }
  };

  const restore = async (item: BackupMeta) => {
    const confirmed = window.confirm(
      `从「${item.filename}」恢复？恢复会覆盖共享库与全部账本文件，并使所有会话失效。`
    );
    if (!confirmed) return;
    setBusyId(item.id);
    setError(null);
    try {
      await restoreBackup(item.id);
      flash("备份已恢复，所有会话已失效，即将重新登录…");
      window.setTimeout(() => window.location.reload(), 900);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "恢复备份失败");
      setBusyId(null);
    }
  };

  return (
    <div className="page page-enter">
      <PageTitle
        eyebrow="SYSTEM"
        title="系统管理"
        actions={
          <button className="primary-button" onClick={() => void create()} disabled={creating}>
            {creating ? <LoaderCircle className="spin" size={18} /> : <Plus size={18} />}
            {creating ? "备份中…" : "立即备份"}
          </button>
        }
      />
      {success && <div className="users-success" role="status">{success}</div>}
      {error && <div className="inline-error">{error}</div>}
      <div className="backup-note">
        <ShieldAlert size={17} />
        <span>备份包含共享库与全部账本文件。恢复会<strong>覆盖</strong>当前数据并使所有登录会话失效，请谨慎操作。</span>
      </div>
      <article className="panel transaction-table">
        <div className="table-header"><span>备份</span><span>创建时间</span><span>大小</span><span>包含文件</span><span /><span /></div>
        {backups === null ? (
          <div className="empty-hint"><LoaderCircle className="spin" size={18} /> 正在加载…</div>
        ) : backups.length === 0 ? (
          <EmptyState title="还没有备份" detail="点击「立即备份」创建第一份快照。" />
        ) : (
          backups.map((item) => (
            <div className="transaction-row" key={item.id}>
              <div className="transaction-main">
                <span className="transaction-icon transfer"><DatabaseBackup size={18} /></span>
                <div>
                  <strong>{item.filename}</strong>
                  <span className="transaction-meta">
                    <span>{item.id}</span>
                  </span>
                </div>
              </div>
              <span className="table-account">{formatDate(item.created_at)}</span>
              <span className="table-date">{formatBytes(item.size_bytes)}</span>
              <span className="table-date" title={item.files.join("\n")}>
                {item.files.length} 个
              </span>
              <div className="row-menu-wrap">
                <button
                  className="row-action"
                  onClick={() => void download(item)}
                  disabled={busyId === item.id}
                  title="下载备份"
                  aria-label="下载备份"
                >
                  {busyId === item.id ? <LoaderCircle className="spin" size={16} /> : <Download size={16} />}
                </button>
              </div>
              <div className="row-menu-wrap">
                <button
                  className="row-action danger"
                  onClick={() => void restore(item)}
                  disabled={busyId === item.id}
                  title="从该备份恢复（覆盖全部数据）"
                  aria-label="恢复备份"
                >
                  <RotateCcw size={16} />
                </button>
              </div>
            </div>
          ))
        )}
      </article>
    </div>
  );
}
