//! 报销弹窗：对可报销支出生成报销入账。
import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import { RateHintLine, useRateHint } from "../../components/RateHint";
import { formatMoney } from "../../lib";
import type { Account, Transaction } from "../../types";

export function ReimburseModal({
  expense,
  accounts,
  onClose,
  onSubmit
}: {
  expense: Transaction;
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
  const remaining = Math.max(0, Number(expense.amount) - Number(expense.reimbursed_amount) - Number(expense.refunded_amount)).toFixed(2);
  const [accountId, setAccountId] = useState("");
  const [amount, setAmount] = useState(remaining);
  const [settledAmount, setSettledAmount] = useState("");
  const [settledTouched, setSettledTouched] = useState(false);
  const [note, setNote] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const selectedAccount = accounts.find((account) => account.id === Number(accountId));
  const crossCurrency = selectedAccount != null && selectedAccount.currency !== expense.currency;
  const { hint, status, refresh } = useRateHint(
    crossCurrency ? expense.currency : null,
    crossCurrency ? selectedAccount?.currency ?? null : null
  );
  // 汇率就绪后用真实汇率替换 1:1 预填值（用户手动改过则不覆盖）。
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
      // 报销币种始终与支出一致；到账账户币种不同时需要显式给出入账金额。
      if (crossCurrency) {
        input.currency = expense.currency;
        input.settled_amount = settledAmount;
      }
      await onSubmit(input);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("common.opFailed"));
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="REIMBURSEMENT" title={t("modals.reimburse.title")} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="deposit-info">
          <p>{t("modals.reimburse.info", { note: expense.note || t("modals.reimburse.defaultNote"), amount: formatMoney(expense.amount, expense.currency), remaining: formatMoney(remaining, expense.currency) })}</p>
        </div>
        <div className="form-grid">
          <label><span>{t("modals.reimburse.account")}</span>
            <select
              required
              value={accountId}
              onChange={(e) => {
                const nextId = e.target.value;
                setAccountId(nextId);
                const next = accounts.find((account) => account.id === Number(nextId));
                if (next != null && next.currency !== expense.currency && settledAmount === "") {
                  setSettledAmount(amount);
                }
              }}
            >
              <option value="" disabled>{t("common.selectAccount")}</option>
              {accounts.map((account) => (
                <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>
              ))}
            </select>
          </label>
          <label><span>{t("modals.reimburse.amount", { currency: expense.currency })}</span><input required step="0.01" inputMode="decimal" max={remaining} value={amount} onChange={(e) => setAmount(e.target.value)} /></label>
          {crossCurrency && (
            <>
              <label className="span-two"><span>{t("modals.reimburse.settled", { currency: selectedAccount.currency })}</span>
                <input
                  required
                  step="0.01"
                  inputMode="decimal"
                  value={settledAmount}
                  onChange={(e) => {
                    setSettledTouched(true);
                    setSettledAmount(e.target.value);
                  }}
                  placeholder={t("modals.reimburse.settledPlaceholder", { currency: selectedAccount.currency })}
                />
              </label>
              <div className="span-two">
                <RateHintLine from={expense.currency} to={selectedAccount.currency} status={status} hint={hint} onRefresh={refresh} />
              </div>
            </>
          )}
          <label className="span-two"><span>{t("common.note")}</span><input value={note} onChange={(e) => setNote(e.target.value)} placeholder={t("common.optional")} /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
          <button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}{t("modals.reimburse.submit")}</button>
        </div>
      </form>
    </ModalShell>
  );
}
