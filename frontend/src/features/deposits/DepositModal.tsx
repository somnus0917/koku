//! 新建定期存款弹窗。
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import { formatMoney } from "../../lib";
import type { Account } from "../../types";

export function DepositModal({
  source,
  onClose,
  onSubmit
}: {
  source: Account;
  onClose: () => void;
  onSubmit: (input: { amount: string; rate: string; term_days: number; note?: string }) => Promise<void>;
}) {
  const [amount, setAmount] = useState("");
  const [rate, setRate] = useState("");
  const [termDays, setTermDays] = useState("90");
  const [note, setNote] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      await onSubmit({ amount, rate, term_days: Number(termDays), note: note || undefined });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("common.opFailed"));
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="FIXED DEPOSIT" title={t("modals.deposit.title")} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="deposit-info">
          <p>{t("modals.deposit.infoPrefix")}<strong>{source.name}</strong>{t("modals.deposit.infoSuffix", { currency: source.currency, balance: formatMoney(source.balance, source.currency) })}</p>
        </div>
        <div className="form-grid">
          <label><span>{t("modals.deposit.amount")}</span><input required autoFocus step="0.01" inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="0.00" /></label>
          <label><span>{t("modals.deposit.rate")}</span><input required step="0.01" inputMode="decimal" value={rate} onChange={(e) => setRate(e.target.value)} placeholder={t("modals.deposit.ratePlaceholder")} /></label>
          <label><span>{t("modals.deposit.termDays")}</span><input required type="number" min={1} value={termDays} onChange={(e) => setTermDays(e.target.value)} /></label>
          <label className="span-two"><span>{t("common.note")}</span><input value={note} onChange={(e) => setNote(e.target.value)} placeholder={t("common.optional")} /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
          <button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}{t("modals.deposit.submit")}</button>
        </div>
      </form>
    </ModalShell>
  );
}
