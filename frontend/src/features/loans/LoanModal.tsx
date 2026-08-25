//! 新建借款/出借弹窗。
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import type { Account, LoanType } from "../../types";

export function LoanModal({
  accounts,
  counterparties,
  onClose,
  onSubmit
}: {
  accounts: Account[];
  /** 历史往来人（来自已有借款），下拉可选；选中已有的人会合并到未结清的同一方向借款 */
  counterparties: string[];
  onClose: () => void;
  onSubmit: (input: { loan_type: LoanType; counterparty: string; amount: string; account_id: number; interest_rate?: string; note?: string; due_at?: string }) => Promise<void>;
}) {
  const [loanType, setLoanType] = useState<LoanType>("lend");
  const [counterparty, setCounterparty] = useState("");
  const [accountId, setAccountId] = useState("");
  const [amount, setAmount] = useState("");
  const [interestRate, setInterestRate] = useState("");
  const [note, setNote] = useState("");
  const [dueAt, setDueAt] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      await onSubmit({
        loan_type: loanType,
        counterparty,
        amount,
        interest_rate: interestRate || undefined,
        account_id: Number(accountId),
        note: note || undefined,
        due_at: dueAt ? new Date(`${dueAt}T00:00:00`).toISOString() : undefined
      });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("common.opFailed"));
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="LOAN" title={t("accounts.loans.add")} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="form-grid">
          <label><span>{t("modals.loan.direction")}</span>
            <select value={loanType} onChange={(e) => setLoanType(e.target.value as LoanType)}>
              <option value="lend">{t("modals.loan.lendOption")}</option>
              <option value="borrow">{t("modals.loan.borrowOption")}</option>
            </select>
          </label>
          <label><span>{t("modals.loan.counterparty")}</span>
            <input required autoFocus list="koku-counterparties" value={counterparty} onChange={(e) => setCounterparty(e.target.value)} placeholder={t("modals.loan.counterpartyPlaceholder")} />
            <datalist id="koku-counterparties">
              {counterparties.map((name) => <option key={name} value={name} />)}
            </datalist>
          </label>
          <label><span>{t("common.amount")}</span><input required step="0.01" inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="0.00" /></label>
          <label><span>{t("modals.loan.interestRate")}</span><input min="0" step="0.01" inputMode="decimal" value={interestRate} onChange={(e) => setInterestRate(e.target.value)} placeholder={t("modals.loan.interestRatePlaceholder")} /></label>
          <label><span>{t("common.fundingAccount")}</span>
            <select required value={accountId} onChange={(e) => setAccountId(e.target.value)}>
              <option value="" disabled>{t("common.selectAccount")}</option>
              {accounts.map((account) => (
                <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>
              ))}
            </select>
          </label>
          <label className="span-two"><span>{t("common.note")}</span><input value={note} onChange={(e) => setNote(e.target.value)} placeholder={t("common.optional")} /></label>
          <label><span>{t("modals.loan.dueDate")}</span><input type="date" value={dueAt} onChange={(e) => setDueAt(e.target.value)} /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
          <button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}{t(loanType === "lend" ? "modals.loan.confirmLend" : "modals.loan.confirmBorrow")}</button>
        </div>
      </form>
    </ModalShell>
  );
}
