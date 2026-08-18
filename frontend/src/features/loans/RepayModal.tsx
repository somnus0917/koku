//! 还款弹窗（借款/出借通用，含跨币种折算）。
import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import { RateHintLine, useRateHint } from "../../components/RateHint";
import { formatMoney } from "../../lib";
import type { Account, Loan } from "../../types";

export function RepayModal({
  loan,
  accounts,
  onClose,
  onSubmit
}: {
  loan: Loan;
  accounts: Account[];
  onClose: () => void;
  onSubmit: (input: {
    account_id: number;
    amount: string;
    note?: string;
    currency?: string;
    settled_amount?: string;
  }) => Promise<void>;
}) {
  const [accountId, setAccountId] = useState("");
  const [amount, setAmount] = useState(loan.outstanding);
  const [settledAmount, setSettledAmount] = useState("");
  const [settledTouched, setSettledTouched] = useState(false);
  const [note, setNote] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const selectedAccount = accounts.find((account) => account.id === Number(accountId));
  const crossCurrency = selectedAccount != null && selectedAccount.currency !== loan.currency;
  const { hint, status, refresh } = useRateHint(
    crossCurrency ? loan.currency : null,
    crossCurrency ? selectedAccount?.currency ?? null : null
  );
  // 汇率就绪后用真实汇率预填折算金额（用户手动改过则不覆盖）。
  useEffect(() => {
    if (crossCurrency && status === "ok" && hint && !settledTouched) {
      setSettledAmount((Number(amount) * Number(hint.rate)).toFixed(2));
    }
  }, [crossCurrency, status, hint, amount, settledTouched]);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      const input: {
        account_id: number;
        amount: string;
        note?: string;
        currency?: string;
        settled_amount?: string;
      } = { account_id: Number(accountId), amount, note: note || undefined };
      // 还款币种始终与借款币种一致；资金账户币种不同时需要显式给出入账金额。
      if (crossCurrency) {
        input.currency = loan.currency;
        input.settled_amount = settledAmount;
      }
      await onSubmit(input);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("common.opFailed"));
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="REPAYMENT" title={t(loan.loan_type === "lend" ? "modals.repay.titleLend" : "modals.repay.titleBorrow")} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="deposit-info">
          <p>{t(loan.loan_type === "lend" ? "modals.repay.infoLend" : "modals.repay.infoBorrow", { name: loan.counterparty, amount: formatMoney(loan.outstanding, loan.currency) })}</p>
        </div>
        <div className="form-grid">
          <label><span>{t("common.fundingAccount")}</span>
            <select required value={accountId} onChange={(e) => setAccountId(e.target.value)}>
              <option value="" disabled>{t("common.selectAccount")}</option>
              {accounts.map((account) => (
                <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>
              ))}
            </select>
          </label>
          <label><span>{t("modals.repay.amount", { currency: loan.currency })}</span><input required step="0.01" inputMode="decimal" max={loan.outstanding} value={amount} onChange={(e) => setAmount(e.target.value)} /></label>
          {crossCurrency && (
            <>
              <label className="span-two"><span>{t("modals.repay.settled", { currency: selectedAccount.currency })}</span>
                <input
                  required
                  min="0.01"
                  step="0.01"
                  inputMode="decimal"
                  value={settledAmount}
                  onChange={(e) => {
                    setSettledTouched(true);
                    setSettledAmount(e.target.value);
                  }}
                  placeholder={t("modals.repay.settledPlaceholder", { currency: selectedAccount.currency })}
                />
              </label>
              <div className="span-two">
                <RateHintLine from={loan.currency} to={selectedAccount.currency} status={status} hint={hint} onRefresh={refresh} />
              </div>
            </>
          )}
          <label className="span-two"><span>{t("common.note")}</span><input value={note} onChange={(e) => setNote(e.target.value)} placeholder={t("common.optional")} /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
          <button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}{t("modals.repay.submit")}</button>
        </div>
      </form>
    </ModalShell>
  );
}
