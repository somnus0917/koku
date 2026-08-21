//! 新建周期交易弹窗。
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import type { Account, Category, RecurrenceFrequency, RecurringRule } from "../../types";

export function RecurringModal({
  accounts,
  categories,
  onClose,
  onSubmit,
  rule
}: {
  accounts: Account[];
  categories: Category[];
  onClose: () => void;
  onSubmit: (input: {
    kind: "expense" | "income";
    account_id: number;
    category_id: number;
    amount: string;
    note?: string;
    frequency: RecurrenceFrequency;
    next_due_at: string;
  }) => Promise<void>;
  rule?: RecurringRule | null;
}) {
  const [kind, setKind] = useState<"expense" | "income">(rule?.kind === "income" ? "income" : "expense");
  const [accountId, setAccountId] = useState(rule?.account_id ?? accounts[0]?.id ?? 0);
  const [categoryId, setCategoryId] = useState(rule ? String(rule.category_id) : "");
  const [amount, setAmount] = useState(rule?.amount ?? "");
  const [note, setNote] = useState(rule?.note ?? "");
  const [frequency, setFrequency] = useState<RecurrenceFrequency>(rule?.frequency ?? "monthly");
  const [startDate, setStartDate] = useState(rule?.next_due_at.slice(0, 10) ?? "");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const kindCategories = categories.filter((category) => category.kind === kind);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      const next_due_at = new Date(`${startDate}T00:00:00`).toISOString();
      await onSubmit({
        kind,
        account_id: Number(accountId),
        category_id: Number(categoryId),
        amount,
        note: note || undefined,
        frequency,
        next_due_at
      });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("common.opFailed"));
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="RECURRING" title={rule ? t("modals.recurring.editTitle") : t("modals.recurring.title")} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="form-grid">
          <label><span>{t("modals.recurring.kind")}</span>
            <select value={kind} onChange={(event) => { setKind(event.target.value as "expense" | "income"); setCategoryId(""); }}>
              <option value="expense">{t("common.expense")}</option>
              <option value="income">{t("common.income")}</option>
            </select>
          </label>
          <label><span>{t("common.amount")}</span><input required step="0.01" inputMode="decimal" value={amount} onChange={(event) => setAmount(event.target.value)} placeholder="0.00" /></label>
          <label><span>{t("common.fundingAccount")}</span>
            <select required value={accountId} onChange={(event) => setAccountId(Number(event.target.value))}>
              {accounts.map((account) => (
                <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>
              ))}
            </select>
          </label>
          <label><span>{t("common.category")}</span>
            <select required value={categoryId} onChange={(event) => setCategoryId(event.target.value)}>
              <option value="" disabled>{t("modals.recurring.selectCategory")}</option>
              {kindCategories.map((category) => (
                <option key={category.id} value={category.id}>{category.name}</option>
              ))}
            </select>
          </label>
          <label><span>{t("modals.recurring.frequency")}</span>
            <select value={frequency} onChange={(event) => setFrequency(event.target.value as RecurrenceFrequency)}>
              <option value="monthly">{t("common.monthly")}</option>
              <option value="weekly">{t("common.weekly")}</option>
            </select>
          </label>
          <label><span>{t("modals.recurring.startDate")}</span><input required type="date" value={startDate} onChange={(event) => setStartDate(event.target.value)} /></label>
          <label className="span-two"><span>{t("common.note")}</span><input value={note} onChange={(event) => setNote(event.target.value)} placeholder={t("modals.recurring.notePlaceholder")} /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
          <button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}{rule ? t("modals.recurring.save") : t("common.create")}</button>
        </div>
      </form>
    </ModalShell>
  );
}
