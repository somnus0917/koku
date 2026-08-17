//! 管理员系统页：备份/恢复（仅 admin 角色可见，非 admin 请求会被后端 403）。

import { useEffect, useRef, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
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
  const { t } = useTranslation();

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
      setError(reason instanceof Error ? reason.message : t("system.loadFailed"));
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
      flash(t("system.created"));
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("system.createFailed"));
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
      setError(reason instanceof Error ? reason.message : t("system.downloadFailed"));
    } finally {
      setBusyId(null);
    }
  };

  const restore = async (item: BackupMeta) => {
    const confirmed = window.confirm(
      t("system.confirmRestore", { filename: item.filename })
    );
    if (!confirmed) return;
    setBusyId(item.id);
    setError(null);
    try {
      await restoreBackup(item.id);
      flash(t("system.restored"));
      window.setTimeout(() => window.location.reload(), 900);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("system.restoreFailed"));
      setBusyId(null);
    }
  };

  return (
    <div className="page page-enter">
      <PageTitle
        eyebrow="SYSTEM"
        title={t("system.title")}
        actions={
          <button className="primary-button" onClick={() => void create()} disabled={creating}>
            {creating ? <LoaderCircle className="spin" size={18} /> : <Plus size={18} />}
            {creating ? t("system.creating") : t("system.createNow")}
          </button>
        }
      />
      {success && <div className="users-success" role="status">{success}</div>}
      {error && <div className="inline-error">{error}</div>}
      <div className="backup-note">
        <ShieldAlert size={17} />
        <span><Trans i18nKey="system.note" components={{ strong: <strong /> }} /></span>
      </div>
      <article className="panel transaction-table">
        <div className="table-header"><span>{t("system.colBackup")}</span><span>{t("common.colCreatedAt")}</span><span>{t("system.colSize")}</span><span>{t("system.colFiles")}</span><span /><span /></div>
        {backups === null ? (
          <div className="empty-hint"><LoaderCircle className="spin" size={18} /> {t("common.loading")}</div>
        ) : backups.length === 0 ? (
          <EmptyState title={t("system.emptyTitle")} detail={t("system.emptyDetail")} />
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
                {t("system.fileCount", { count: item.files.length })}
              </span>
              <div className="row-menu-wrap">
                <button
                  className="row-action"
                  onClick={() => void download(item)}
                  disabled={busyId === item.id}
                  title={t("system.download")}
                  aria-label={t("system.download")}
                >
                  {busyId === item.id ? <LoaderCircle className="spin" size={16} /> : <Download size={16} />}
                </button>
              </div>
              <div className="row-menu-wrap">
                <button
                  className="row-action danger"
                  onClick={() => void restore(item)}
                  disabled={busyId === item.id}
                  title={t("system.restoreTitle")}
                  aria-label={t("system.restoreAria")}
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
