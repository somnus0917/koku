//! 管理员用户管理页：查看用户、创建成员、重置密码、启用/停用、删除（连带账本）。

import { useEffect, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import {
  KeyRound,
  LoaderCircle,
  Plus,
  Shield,
  ShieldOff,
  Trash2,
  UserRound,
  X
} from "lucide-react";
import {
  createUser,
  deleteUser,
  listUsers,
  resetUserPassword,
  setUserEnabled
} from "../api";
import { formatDate } from "../lib";
import { PageTitle } from "./PageTitle";
import type { User } from "../types";

/** 密码输入弹窗：新建用户（含邮箱）或重置密码。 */
function UserPasswordModal({
  mode,
  existing,
  onClose,
  onSubmit
}: {
  mode: "create" | "reset";
  existing?: User;
  onClose: () => void;
  onSubmit: (email: string, password: string) => Promise<void>;
}) {
  const [email, setEmail] = useState(existing?.email ?? "");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const { t } = useTranslation();
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      await onSubmit(email.trim(), password);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("common.opFailed"));
      setSubmitting(false);
    }
  };
  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section className="modal-card" role="dialog" aria-modal="true" aria-label={mode === "create" ? t("users.titleCreate") : t("users.resetPassword")}>
        <header>
          <div><span>{mode === "create" ? "NEW USER" : "RESET PASSWORD"}</span><h2>{mode === "create" ? t("users.titleCreate") : t("users.titleReset", { email: existing?.email })}</h2></div>
          <button className="icon-button" onClick={onClose}><X size={19} /></button>
        </header>
        <form className="entry-form" onSubmit={submit}>
          <div className="form-grid">
            {mode === "create" && (
              <label className="span-two"><span>{t("users.email")}</span>
                <input required autoFocus type="email" autoComplete="email" value={email} onChange={(e) => setEmail(e.target.value)} placeholder={t("users.emailPlaceholder")} />
              </label>
            )}
            <label className="span-two"><span>{t("users.initialPassword")}</span>
              <input required autoFocus={mode === "reset"} type="password" value={password} onChange={(e) => setPassword(e.target.value)} placeholder="••••••••" />
            </label>
          </div>
          {error && <div className="form-error">{error}</div>}
          <div className="modal-actions">
            <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
            <button className="primary-button" disabled={submitting || !password || (mode === "create" && !email)}>
              {submitting && <LoaderCircle className="spin" size={17} />}{mode === "create" ? t("users.create") : t("users.resetPassword")}
            </button>
          </div>
        </form>
      </section>
    </div>
  );
}

export function UsersAdminPage({ currentUserId }: { currentUserId: number }) {
  const [users, setUsers] = useState<User[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const successTimer = useRef<number | undefined>(undefined);
  const [passwordModal, setPasswordModal] = useState<{ mode: "create" | "reset"; user?: User } | null>(null);
  const [busyId, setBusyId] = useState<number | null>(null);
  const { t } = useTranslation();

  const flash = (message: string) => {
    setSuccess(message);
    window.clearTimeout(successTimer.current);
    successTimer.current = window.setTimeout(() => setSuccess(null), 2600);
  };

  const refresh = async () => {
    try {
      setUsers(await listUsers());
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("users.loadFailed"));
    }
  };
  useEffect(() => {
    void refresh();
  }, []);

  const act = async (action: () => Promise<unknown>, message: string) => {
    try {
      await action();
      flash(message);
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("common.opFailed"));
    }
  };

  const submitPassword = async (email: string, password: string) => {
    if (passwordModal?.mode === "create") {
      await createUser(email, password);
      flash(t("users.created", { email }));
    } else if (passwordModal?.user) {
      await resetUserPassword(passwordModal.user.id, password);
      flash(t("users.passwordReset", { email: passwordModal.user.email }));
    }
    setPasswordModal(null);
    await refresh();
  };

  const remove = async (target: User) => {
    if (!window.confirm(t("users.confirmDelete", { email: target.email }))) return;
    setBusyId(target.id);
    await act(() => deleteUser(target.id), t("users.deleted", { email: target.email }));
    setBusyId(null);
  };

  return (
    <div className="page page-enter">
      <PageTitle
        eyebrow="USERS"
        title={t("users.pageTitle")}
        actions={<button className="primary-button" onClick={() => setPasswordModal({ mode: "create" })}><Plus size={18} /> {t("users.titleCreate")}</button>}
      />
      {success && <div className="users-success" role="status">{success}</div>}
      {error && <div className="inline-error">{error}</div>}
      <article className="panel transaction-table">
        <div className="table-header"><span>{t("users.colUser")}</span><span>{t("users.colRole")}</span><span>{t("users.colStatus")}</span><span>{t("common.colCreatedAt")}</span><span /><span /></div>
        {users === null ? (
          <div className="empty-hint"><LoaderCircle className="spin" size={18} /> {t("common.loading")}</div>
        ) : users.length === 0 ? (
          <div className="empty-hint">{t("users.empty")}</div>
        ) : (
          users.map((item) => (
            <div className="transaction-row" key={item.id}>
              <div className="transaction-main">
                <span className="transaction-icon transfer"><UserRound size={18} /></span>
                <div>
                  <strong>{item.email}{item.id === currentUserId ? t("users.currentAccount") : ""}</strong>
                  <span className="transaction-meta"><span>{item.role === "admin" ? t("users.admin") : t("users.member")}</span></span>
                </div>
              </div>
              <span className="table-account">{item.role === "admin" ? <Shield size={15} /> : <ShieldOff size={15} />}</span>
              <span className="table-date">{item.enabled ? t("users.enabled") : t("users.disabled")}</span>
              <span className="table-date">{formatDate(item.created_at)}</span>
              <div className="row-menu-wrap">
                <button
                  className="row-action"
                  title={item.enabled ? t("users.disableTitle") : t("users.enableTitle")}
                  aria-label={item.enabled ? t("users.disableAria") : t("users.enableAria")}
                  disabled={item.id === currentUserId}
                  onClick={() => {
                    setBusyId(item.id);
                    void act(() => setUserEnabled(item.id, !item.enabled), item.enabled ? t("users.disabledFlash", { email: item.email }) : t("users.enabledFlash", { email: item.email })).then(() => setBusyId(null));
                  }}
                >{busyId === item.id ? <LoaderCircle className="spin" size={16} /> : item.enabled ? <ShieldOff size={16} /> : <Shield size={16} />}</button>
              </div>
              <div className="row-menu-wrap">
                <button className="row-action" title={t("users.resetPassword")} aria-label={t("users.resetPassword")} onClick={() => setPasswordModal({ mode: "reset", user: item })}>
                  <KeyRound size={16} />
                </button>
              </div>
              <div className="row-menu-wrap">
                <button
                  className="row-action"
                  title={t("users.delete")}
                  aria-label={t("users.delete")}
                  disabled={item.id === currentUserId}
                  onClick={() => void remove(item)}
                ><Trash2 size={16} /></button>
              </div>
            </div>
          ))
        )}
      </article>
      {passwordModal && (
        <UserPasswordModal
          mode={passwordModal.mode}
          existing={passwordModal.user}
          onClose={() => setPasswordModal(null)}
          onSubmit={submitPassword}
        />
      )}
    </div>
  );
}
