//! 已撤销流水的永久删除确认弹窗。
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle, TriangleAlert } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import { formatMoney } from "../../lib";
import type { Transaction } from "../../types";

export function DeleteTransactionModal({
  transaction,
  onClose,
  onConfirm
}: {
  transaction: Transaction;
  onClose: () => void;
  onConfirm: () => Promise<void>;
}) {
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const label = transaction.note || t(`transactions.meta.${transaction.kind}`);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      await onConfirm();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("modals.deleteTransaction.failed"));
      setSubmitting(false);
    }
  };

  return (
    <ModalShell eyebrow="PERMANENT DELETE" title={t("modals.deleteTransaction.title")} onClose={onClose}>
      <form className="entry-form" onSubmit={(event) => void submit(event)}>
        <div className="permanent-delete-warning">
          <TriangleAlert size={20} />
          <div>
            <strong>{label}</strong>
            <span>{formatMoney(transaction.amount, transaction.currency)}</span>
            <p>{t("modals.deleteTransaction.warning")}</p>
          </div>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" disabled={submitting} onClick={onClose}>{t("common.cancel")}</button>
          <button className="danger-button" disabled={submitting}>
            {submitting && <LoaderCircle className="spin" size={17} />}
            {t("transactions.deletePermanent")}
          </button>
        </div>
      </form>
    </ModalShell>
  );
}
