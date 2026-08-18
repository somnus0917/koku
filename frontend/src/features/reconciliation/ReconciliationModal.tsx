//! 账户对账弹窗：查看对账历史、新建对账、完成/取消进行中的对账。
import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { ClipboardCheck, LoaderCircle, RotateCcw, X } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import { cancelReconciliation, completeReconciliation, createReconciliation, listReconciliations } from "../../api";
import { formatDate, formatMoney } from "../../lib";
import { uiLocale } from "../../i18n";
import type { Account, Reconciliation, ReconciliationStatus } from "../../types";

function todayDateValue(): string {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
}

/** 把 YYYY-MM-DD（或 RFC3339）解析为本地日期。 */
function parseDay(value: string): Date {
  return /^\d{4}-\d{2}-\d{2}$/.test(value) ? new Date(`${value}T00:00:00`) : new Date(value);
}

/** 日期展示（如 "2026年8月15日"），随界面语言变化。 */
function formatDay(value: string): string {
  return new Intl.DateTimeFormat(uiLocale(), { year: "numeric", month: "long", day: "numeric" }).format(parseDay(value));
}

function ReconciliationStatusBadge({ status }: { status: ReconciliationStatus }) {
  const { t } = useTranslation();
  const label = status === "open" ? t("reconcile.statusOpen") : status === "completed" ? t("reconcile.statusCompleted") : t("reconcile.statusCancelled");
  return <span className={`reconcile-status ${status}`}>{label}</span>;
}

/** 账户对账弹窗：查看对账历史、新建对账、完成/取消进行中的对账。 */
export function ReconciliationModal({
  account,
  onClose,
  onChanged
}: {
  account: Account;
  onClose: () => void;
  /** 完成对账后回调：父级刷新余额并提示。 */
  onChanged: () => void;
}) {
  const [items, setItems] = useState<Reconciliation[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [date, setDate] = useState(todayDateValue);
  const [balance, setBalance] = useState("");
  const [note, setNote] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [busyId, setBusyId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();

  const refresh = async () => {
    try {
      setItems(await listReconciliations(account.id));
      setLoadError(null);
    } catch (reason) {
      setLoadError(reason instanceof Error ? reason.message : t("reconcile.loadFailed"));
    }
  };
  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [account.id]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      await createReconciliation({
        account_id: account.id,
        statement_date: date,
        statement_balance: balance,
        note: note.trim() || undefined
      });
      setBalance("");
      setNote("");
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("reconcile.createFailed"));
    } finally {
      setSubmitting(false);
    }
  };

  const complete = async (item: Reconciliation) => {
    setBusyId(item.id); setError(null);
    try {
      await completeReconciliation(item.id);
      await onChanged();
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("reconcile.completeFailed"));
    } finally {
      setBusyId(null);
    }
  };

  const cancel = async (item: Reconciliation) => {
    setBusyId(item.id); setError(null);
    try {
      await cancelReconciliation(item.id);
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("reconcile.cancelFailed"));
    } finally {
      setBusyId(null);
    }
  };

  return (
    <ModalShell eyebrow="RECONCILE" title={t("reconcile.title", { name: account.name })} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="deposit-info">
          <p>{t("reconcile.intro", { amount: formatMoney(account.balance, account.currency) })}</p>
        </div>
        <div className="form-grid">
          <label><span>{t("reconcile.date")}</span><input required type="date" value={date} onChange={(e) => setDate(e.target.value)} /></label>
          <label><span>{t("reconcile.statementBalance", { currency: account.currency })}</span><input required step="0.01" inputMode="decimal" value={balance} onChange={(e) => setBalance(e.target.value)} placeholder="0.00" /></label>
          <label className="span-two"><span>{t("reconcile.note")}</span><input value={note} onChange={(e) => setNote(e.target.value)} placeholder={t("reconcile.notePlaceholder")} /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>{t("common.close")}</button>
          <button className="primary-button" disabled={submitting || !date || !balance}>
            {submitting && <LoaderCircle className="spin" size={17} />}{t("reconcile.create")}
          </button>
        </div>
      </form>

      <div className="reconcile-history">
        <div className="reconcile-history-head"><strong>{t("reconcile.history")}</strong><small>{items ? t("reconcile.count", { count: items.length }) : ""}</small></div>
        {loadError && <div className="form-error">{loadError}</div>}
        {items === null ? (
          loadError ? null : <div className="empty-hint"><LoaderCircle className="spin" size={16} /> {t("common.loading")}</div>
        ) : items.length === 0 ? (
          <div className="empty-hint">{t("reconcile.empty")}</div>
        ) : (
          <div className="reconcile-list">
            {items.map((item) => (
              <div className={`reconcile-item ${item.status}`} key={item.id}>
                <div className="reconcile-item-head">
                  <strong>{formatDay(item.statement_date)}</strong>
                  <ReconciliationStatusBadge status={item.status} />
                </div>
                <div className="reconcile-item-meta">
                  <span>{t("reconcile.statementLabel", { amount: formatMoney(item.statement_balance, account.currency) })}</span>
                  <span>{t("reconcile.bookLabel", { amount: formatMoney(item.book_balance, account.currency) })}</span>
                  <span>{t("reconcile.openedAt", { date: formatDate(item.opened_at) })}</span>
                </div>
                {item.note && <p className="fx-hint">{item.note}</p>}
                {item.completed_at && <p className="fx-hint">{t("reconcile.completedAt", { date: formatDate(item.completed_at) })}</p>}
                {item.adjustment_transaction_id != null && (
                  <p className="reconcile-adjustment"><RotateCcw size={12} /> {t("reconcile.adjustmentNote")}</p>
                )}
                {item.status === "open" && (
                  <div className="reconcile-actions">
                    <button type="button" className="text-button" disabled={busyId === item.id} onClick={() => void complete(item)}>
                      {busyId === item.id ? <LoaderCircle className="spin" size={13} /> : <ClipboardCheck size={13} />}{t("reconcile.complete")}
                    </button>
                    <button type="button" className="text-button danger" disabled={busyId === item.id} onClick={() => void cancel(item)}>
                      <X size={13} />{t("common.cancel")}
                    </button>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </ModalShell>
  );
}
