//! 定期到期结算弹窗。
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import { formatDate, formatMoney } from "../../lib";
import type { Account, Deposit } from "../../types";

export function SettleDepositModal({
  deposit,
  accounts,
  onClose,
  onSubmit
}: {
  deposit: Deposit;
  accounts: Account[];
  onClose: () => void;
  onSubmit: (toAccountId: number) => Promise<void>;
}) {
  const [targetId, setTargetId] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      await onSubmit(Number(targetId));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("common.opFailed"));
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="MATURE DEPOSIT" title={t("modals.settleDeposit.title")} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="deposit-info">
          <p><strong>{t("deposit.term", { days: deposit.term_days })}</strong>{t("deposit.rate", { rate: deposit.rate })}{deposit.maturity_at ? t("deposit.maturesOn", { date: formatDate(deposit.maturity_at) }) : ""}</p>
          <p>{t("modals.settleDeposit.info", { amount: formatMoney(deposit.amount, deposit.currency) })}</p>
        </div>
        <div className="form-grid">
          <label className="span-two"><span>{t("modals.settleDeposit.targetAccount")}</span>
            <select required value={targetId} onChange={(e) => setTargetId(e.target.value)}>
              <option value="" disabled>{t("modals.settleDeposit.selectTarget")}</option>
              {accounts.map((account) => (
                <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>
              ))}
            </select>
          </label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
          <button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}{t("modals.settleDeposit.submit")}</button>
        </div>
      </form>
    </ModalShell>
  );
}
