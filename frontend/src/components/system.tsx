//! 管理员系统页：备份/恢复 + 异地备份（R2）状态与操作
//! （仅 admin 角色可见，非 admin 请求会被后端 403）。

import { useEffect, useRef, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import {
  CloudUpload,
  DatabaseBackup,
  Download,
  LoaderCircle,
  Plus,
  RotateCcw,
  ShieldAlert,
  Trash2,
  UploadCloud
} from "lucide-react";
import {
  createBackup,
  downloadBackup,
  listBackups,
  r2Delete,
  r2Restore,
  r2Status,
  r2Upload,
  restoreBackup
} from "../api";
import { formatBytes, formatDate } from "../lib";
import { EmptyState } from "./EmptyState";
import { PageTitle } from "./PageTitle";
import type { BackupMeta, R2Status } from "../types";

export function SystemAdminPage() {
  const [backups, setBackups] = useState<BackupMeta[] | null>(null);
  const [r2, setR2] = useState<R2Status | null>(null);
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

  const loadR2 = async () => {
    try {
      setR2(await r2Status());
    } catch (reason) {
      // R2 状态加载失败不阻塞备份列表。
      setError(reason instanceof Error ? reason.message : t("system.r2StatusLoadFailed"));
    }
  };

  useEffect(() => {
    void refresh();
    void loadR2();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const create = async () => {
    setCreating(true);
    setError(null);
    try {
      await createBackup();
      flash(t("system.created"));
      await refresh();
      await loadR2();
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
    const confirmed = window.confirm(t("system.confirmRestore", { filename: item.filename }));
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

  const uploadToR2 = async (item: BackupMeta) => {
    setBusyId(item.id);
    setError(null);
    try {
      await r2Upload(item.id);
      flash(t("system.r2Uploaded"));
      await refresh();
      await loadR2();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("system.r2UploadFailed"));
    } finally {
      setBusyId(null);
    }
  };

  const deleteFromR2 = async (item: BackupMeta) => {
    const confirmed = window.confirm(t("system.r2DeleteConfirm", { filename: item.filename }));
    if (!confirmed) return;
    setBusyId(item.id);
    setError(null);
    try {
      await r2Delete(item.id);
      flash(t("system.r2Deleted"));
      await refresh();
      await loadR2();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("system.r2DeleteFailed"));
    } finally {
      setBusyId(null);
    }
  };

  const restoreFromR2 = async (item: BackupMeta) => {
    const confirmed = window.confirm(
      t("system.r2RestoreConfirm", { filename: item.filename })
    );
    if (!confirmed) return;
    setBusyId(item.id);
    setError(null);
    try {
      await r2Restore(item.id);
      flash(t("system.r2Restored"));
      window.setTimeout(() => window.location.reload(), 900);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("system.r2RestoreFailed"));
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

      {/* R2 异地备份状态卡片 */}
      <article className="panel r2-panel">
        <div className="section-heading compact-heading">
          <div>
            <span>OFF-SITE BACKUP</span>
            <h2>{t("system.r2Title")}</h2>
          </div>
          <span className={`r2-badge ${r2?.enabled ? "r2-on" : "r2-off"}`}>
            {r2?.enabled ? t("system.r2Enabled") : t("system.r2Disabled")}
          </span>
        </div>
        {r2?.enabled ? (
          <div className="r2-meta">
            <span>{t("system.r2Bucket")}：<strong>{r2.bucket}</strong></span>
            <span>{t("system.r2Prefix")}：<strong>{r2.prefix}</strong></span>
            <span>
              {t("system.r2LastUploaded")}：
              {r2.last_uploaded
                ? t("system.r2LastUploadedAt", {
                    backup_id: r2.last_uploaded.backup_id,
                    size: formatBytes(r2.last_uploaded.size_bytes)
                  })
                : t("system.r2Never")}
            </span>
          </div>
        ) : (
          <div className="r2-meta r2-disabled-hint">
            <CloudUpload size={16} />
            <span>{t("system.r2DisabledHint")}</span>
          </div>
        )}
      </article>

      <div className="backup-note">
        <ShieldAlert size={17} />
        <span><Trans i18nKey="system.note" components={{ strong: <strong /> }} /></span>
      </div>
      <article className="panel transaction-table">
        <div className="table-header">
          <span>{t("system.colBackup")}</span>
          <span>{t("common.colCreatedAt")}</span>
          <span>{t("system.colSize")}</span>
          <span>{t("system.colFiles")}</span>
          <span>R2</span>
          <span />
        </div>
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
              <span className="table-date">
                {item.r2_key ? (
                  <span className="r2-badge r2-on" title={item.r2_key}>{t("system.r2Uploaded")}</span>
                ) : r2?.enabled ? (
                  <span className="r2-badge r2-off">{t("system.r2Never")}</span>
                ) : (
                  <span className="r2-badge r2-off">{t("system.r2Disabled")}</span>
                )}
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
                {r2?.enabled && !item.r2_key && (
                  <button
                    className="row-action"
                    onClick={() => void uploadToR2(item)}
                    disabled={busyId === item.id}
                    title={t("system.r2Upload")}
                    aria-label={t("system.r2Upload")}
                  >
                    {busyId === item.id ? <LoaderCircle className="spin" size={16} /> : <UploadCloud size={16} />}
                  </button>
                )}
                {r2?.enabled && item.r2_key && (
                  <>
                    <button
                      className="row-action"
                      onClick={() => void restoreFromR2(item)}
                      disabled={busyId === item.id}
                      title={t("system.r2Restore")}
                      aria-label={t("system.r2Restore")}
                    >
                      <CloudUpload size={16} />
                    </button>
                    <button
                      className="row-action danger"
                      onClick={() => void deleteFromR2(item)}
                      disabled={busyId === item.id}
                      title={t("system.r2Delete")}
                      aria-label={t("system.r2Delete")}
                    >
                      <Trash2 size={16} />
                    </button>
                  </>
                )}
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
