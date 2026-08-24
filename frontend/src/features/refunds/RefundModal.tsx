//! 退款弹窗：把一笔支出的退款收入记入指定账户，支持部分退款与跨币种入账。
import { useEffect, useState, type FormEvent } from "react";
import { LoaderCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ModalShell } from "../../components/ModalShell";
import { RateHintLine, useRateHint } from "../../components/RateHint";
import { formatMoney } from "../../lib";
import type { Account, Transaction } from "../../types";

export function RefundModal({ expense, accounts, onClose, onSubmit }: {
  expense: Transaction;
  accounts: Account[];
  onClose: () => void;
  onSubmit: (input: { account_id: number; amount: string; note?: string; currency?: string; settled_amount?: string }) => Promise<void>;
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
  const selected = accounts.find((account) => account.id === Number(accountId));
  const crossCurrency = selected != null && selected.currency !== expense.currency;
  const { hint, status, refresh } = useRateHint(crossCurrency ? expense.currency : null, crossCurrency ? selected?.currency ?? null : null);

  useEffect(() => {
    if (crossCurrency && status === "ok" && hint && !settledTouched) {
      setSettledAmount((Number(amount) * Number(hint.rate)).toFixed(2));
    }
  }, [amount, crossCurrency, hint, settledTouched, status]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      const input: { account_id: number; amount: string; note?: string; currency?: string; settled_amount?: string } = {
        account_id: Number(accountId), amount, note: note || undefined
      };
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

  return <ModalShell eyebrow="REFUND" title={t("modals.refund.title")} onClose={onClose}>
    <form className="entry-form" onSubmit={submit}>
      <div className="deposit-info"><p>{t("modals.refund.info", { note: expense.note || t("modals.refund.defaultNote"), amount: formatMoney(expense.amount, expense.currency), remaining: formatMoney(remaining, expense.currency) })}</p></div>
      <div className="form-grid">
        <label><span>{t("modals.refund.account")}</span><select required value={accountId} onChange={(event) => {
          const next = event.target.value; setAccountId(next);
          const account = accounts.find((item) => item.id === Number(next));
          if (account && account.currency !== expense.currency && settledAmount === "") setSettledAmount(amount);
        }}><option value="" disabled>{t("common.selectAccount")}</option>{accounts.map((account) => <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>)}</select></label>
        <label><span>{t("modals.refund.amount", { currency: expense.currency })}</span><input required step="0.01" inputMode="decimal" max={remaining} value={amount} onChange={(event) => setAmount(event.target.value)} /></label>
        {crossCurrency && <><label className="span-two"><span>{t("modals.refund.settled", { currency: selected.currency })}</span><input required step="0.01" inputMode="decimal" value={settledAmount} onChange={(event) => { setSettledTouched(true); setSettledAmount(event.target.value); }} placeholder={t("modals.refund.settledPlaceholder", { currency: selected.currency })} /></label><div className="span-two"><RateHintLine from={expense.currency} to={selected.currency} status={status} hint={hint} onRefresh={refresh} /></div></>}
        <label className="span-two"><span>{t("common.note")}</span><input value={note} onChange={(event) => setNote(event.target.value)} placeholder={t("common.optional")} /></label>
      </div>
      {error && <div className="form-error">{error}</div>}
      <div className="modal-actions"><button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button><button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}{t("modals.refund.submit")}</button></div>
    </form>
  </ModalShell>;
}
