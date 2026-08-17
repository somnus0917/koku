//! 管理员用户管理页：查看用户、创建成员、重置密码、启用/停用、删除（连带账本）。

import { useEffect, useRef, useState, type FormEvent } from "react";
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
import { PageTitle } from "./ledger";
import type { User } from "../types";

/** 密码输入弹窗：新建用户（含用户名）或重置密码。 */
function UserPasswordModal({
  mode,
  existing,
  onClose,
  onSubmit
}: {
  mode: "create" | "reset";
  existing?: User;
  onClose: () => void;
  onSubmit: (username: string, password: string) => Promise<void>;
}) {
  const [username, setUsername] = useState(existing?.username ?? "");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      await onSubmit(username.trim(), password);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "操作失败");
      setSubmitting(false);
    }
  };
  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section className="modal-card" role="dialog" aria-modal="true" aria-label={mode === "create" ? "新建用户" : "重置密码"}>
        <header>
          <div><span>{mode === "create" ? "NEW USER" : "RESET PASSWORD"}</span><h2>{mode === "create" ? "新建用户" : `重置「${existing?.username}」的密码`}</h2></div>
          <button className="icon-button" onClick={onClose}><X size={19} /></button>
        </header>
        <form className="entry-form" onSubmit={submit}>
          <div className="form-grid">
            {mode === "create" && (
              <label className="span-two"><span>用户名</span>
                <input required autoFocus value={username} onChange={(e) => setUsername(e.target.value)} placeholder="例如 alice" />
              </label>
            )}
            <label className="span-two"><span>初始密码（至少 8 位）</span>
              <input required autoFocus={mode === "reset"} type="password" value={password} onChange={(e) => setPassword(e.target.value)} placeholder="••••••••" />
            </label>
          </div>
          {error && <div className="form-error">{error}</div>}
          <div className="modal-actions">
            <button type="button" className="secondary-button" onClick={onClose}>取消</button>
            <button className="primary-button" disabled={submitting || !password || (mode === "create" && !username)}>
              {submitting && <LoaderCircle className="spin" size={17} />}{mode === "create" ? "创建用户" : "重置密码"}
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
      setError(reason instanceof Error ? reason.message : "加载用户列表失败");
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
      setError(reason instanceof Error ? reason.message : "操作失败");
    }
  };

  const submitPassword = async (username: string, password: string) => {
    if (passwordModal?.mode === "create") {
      await createUser(username, password);
      flash(`用户「${username}」已创建`);
    } else if (passwordModal?.user) {
      await resetUserPassword(passwordModal.user.id, password);
      flash(`「${passwordModal.user.username}」的密码已重置`);
    }
    setPasswordModal(null);
    await refresh();
  };

  const remove = async (target: User) => {
    if (!window.confirm(`删除用户「${target.username}」？其独立账本数据将一并删除，不可恢复。`)) return;
    setBusyId(target.id);
    await act(() => deleteUser(target.id), `用户「${target.username}」已删除`);
    setBusyId(null);
  };

  return (
    <div className="page page-enter">
      <PageTitle
        eyebrow="USERS"
        title="用户管理"
        actions={<button className="primary-button" onClick={() => setPasswordModal({ mode: "create" })}><Plus size={18} /> 新建用户</button>}
      />
      {success && <div className="users-success" role="status">{success}</div>}
      {error && <div className="inline-error">{error}</div>}
      <article className="panel transaction-table">
        <div className="table-header"><span>用户</span><span>角色</span><span>状态</span><span>创建时间</span><span /><span /></div>
        {users === null ? (
          <div className="empty-hint"><LoaderCircle className="spin" size={18} /> 正在加载…</div>
        ) : users.length === 0 ? (
          <div className="empty-hint">还没有用户。</div>
        ) : (
          users.map((item) => (
            <div className="transaction-row" key={item.id}>
              <div className="transaction-main">
                <span className="transaction-icon transfer"><UserRound size={18} /></span>
                <div>
                  <strong>{item.username}{item.id === currentUserId ? "（当前账号）" : ""}</strong>
                  <span className="transaction-meta"><span>{item.role === "admin" ? "管理员" : "成员"}</span></span>
                </div>
              </div>
              <span className="table-account">{item.role === "admin" ? <Shield size={15} /> : <ShieldOff size={15} />}</span>
              <span className="table-date">{item.enabled ? "启用" : "已停用"}</span>
              <span className="table-date">{formatDate(item.created_at)}</span>
              <div className="row-menu-wrap">
                <button
                  className="row-action"
                  title={item.enabled ? "停用（作废其会话）" : "启用"}
                  aria-label={item.enabled ? "停用用户" : "启用用户"}
                  disabled={item.id === currentUserId}
                  onClick={() => {
                    setBusyId(item.id);
                    void act(() => setUserEnabled(item.id, !item.enabled), item.enabled ? `「${item.username}」已停用` : `「${item.username}」已启用`).then(() => setBusyId(null));
                  }}
                >{busyId === item.id ? <LoaderCircle className="spin" size={16} /> : item.enabled ? <ShieldOff size={16} /> : <Shield size={16} />}</button>
              </div>
              <div className="row-menu-wrap">
                <button className="row-action" title="重置密码" aria-label="重置密码" onClick={() => setPasswordModal({ mode: "reset", user: item })}>
                  <KeyRound size={16} />
                </button>
              </div>
              <div className="row-menu-wrap">
                <button
                  className="row-action"
                  title="删除用户"
                  aria-label="删除用户"
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
