//! 编辑交易弹窗。
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import { TagEditor } from "./TagEditor";
import { CategoryAvatar } from "../../components/avatar";
import { formatMoney, toLocalDateTimeValue } from "../../lib";
import type { Account, Category, Tag, Transaction } from "../../types";

export function EditTransactionModal({
  transaction,
  accounts,
  categories,
  tags,
  onClose,
  onSubmit
}: {
  transaction: Transaction;
  accounts: Account[];
  categories: Category[];
  tags: Tag[];
  onClose: () => void;
  onSubmit: (input: {
    note?: string;
    occurred_at?: string;
    category_id?: number;
    amount?: string;
    account_id?: number;
    settled_amount?: string;
    tag_names?: string[];
  }) => Promise<void>;
}) {
  const account = accounts.find((item) => item.id === transaction.account_id);
  const accountCurrency = account?.currency ?? transaction.currency;
  const foreign = transaction.currency !== accountCurrency;
  const sameCurrencyAccounts = accounts.filter((item) => item.currency === accountCurrency);
  const matchingCategories = categories.filter((item) => item.kind === transaction.kind);
  // 已发生报销的支出：金额/账户/结算额不可改（报销收入流水由后端兜底拒绝）。
  const reimbursementLocked = transaction.reimbursed_amount !== "0";

  const [note, setNote] = useState(transaction.note);
  const [occurredAt, setOccurredAt] = useState(toLocalDateTimeValue(transaction.occurred_at));
  const [categoryId, setCategoryId] = useState(transaction.category_id ?? 0);
  const [amount, setAmount] = useState(transaction.amount);
  const [settledAmount, setSettledAmount] = useState(transaction.settled_amount);
  const [accountId, setAccountId] = useState(transaction.account_id);
  const [tagNames, setTagNames] = useState<string[]>(transaction.tags);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      const input: {
        note?: string;
        occurred_at?: string;
        category_id?: number;
        amount?: string;
        account_id?: number;
        settled_amount?: string;
        tag_names?: string[];
      } = {};
      if (note !== transaction.note) input.note = note;
      if (occurredAt !== toLocalDateTimeValue(transaction.occurred_at)) {
        input.occurred_at = new Date(occurredAt).toISOString();
      }
      if (categoryId !== (transaction.category_id ?? 0)) input.category_id = categoryId;
      if (accountId !== transaction.account_id) input.account_id = accountId;
      if (amount !== transaction.amount) input.amount = amount;
      // 外币交易：金额与结算额一起提交，保证后端校验通过；同币种结算额恒等于金额。
      if (foreign && (amount !== transaction.amount || settledAmount !== transaction.settled_amount)) {
        input.settled_amount = settledAmount;
      }
      if (tagNames.join(",") !== transaction.tags.join(",")) {
        input.tag_names = tagNames;
      }
      if (Object.keys(input).length === 0) {
        onClose();
        return;
      }
      await onSubmit(input);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("modals.transaction.saveFailed"));
      setSubmitting(false);
    }
  };

  return (
    <ModalShell eyebrow="EDIT ENTRY" title={t("transactions.edit")} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="deposit-info">
          <p>
            {t("modals.editTransaction.info", { kind: t(transaction.kind === "expense" ? "transactions.kind.expense" : "transactions.kind.income"), amount: formatMoney(transaction.amount, transaction.currency) })}
            {foreign ? t("modals.editTransaction.settledSuffix", { amount: formatMoney(transaction.settled_amount, accountCurrency) }) : ""}
          </p>
        </div>
        <div className="form-grid">
          <label><span>{t("common.account")}</span>
            <select
              value={accountId}
              disabled={reimbursementLocked || sameCurrencyAccounts.length <= 1}
              onChange={(e) => setAccountId(Number(e.target.value))}
            >
              {sameCurrencyAccounts.map((item) => (
                <option key={item.id} value={item.id}>{item.name}（{item.currency}）</option>
              ))}
            </select>
          </label>
          <label><span>{t("common.category")}</span>
            <div className="category-input">
              <CategoryAvatar name={matchingCategories.find((item) => item.id === categoryId)?.name ?? t("modals.editTransaction.defaultCategory")} size="small" />
              <select value={categoryId} onChange={(e) => setCategoryId(Number(e.target.value))}>
                {matchingCategories.map((item) => (
                  <option key={item.id} value={item.id}>{item.name}</option>
                ))}
              </select>
            </div>
          </label>
          <label><span>{t("modals.editTransaction.amount", { currency: transaction.currency })}</span>
            <input
              required
              min="0.01"
              step="0.01"
              inputMode="decimal"
              disabled={reimbursementLocked}
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
            />
          </label>
          {foreign && (
            <label><span>{t("modals.editTransaction.settled", { currency: accountCurrency })}</span>
              <input
                required
                min="0.01"
                step="0.01"
                inputMode="decimal"
                disabled={reimbursementLocked}
                value={settledAmount}
                onChange={(e) => setSettledAmount(e.target.value)}
              />
            </label>
          )}
          <label><span>{t("common.time")}</span>
            <input required type="datetime-local" value={occurredAt} onChange={(e) => setOccurredAt(e.target.value)} />
          </label>
          <label className="span-two"><span>{t("common.note")}</span>
            <input value={note} onChange={(e) => setNote(e.target.value)} placeholder={t("common.optional")} />
          </label>
          <label className="span-two"><span>{t("common.tags")}</span>
            <TagEditor value={tagNames} onChange={setTagNames} suggestions={tags.map((tag) => tag.name)} />
          </label>
        </div>
        {reimbursementLocked && <p className="fx-hint">{t("modals.editTransaction.lockedNote")}</p>}
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
          <button className="primary-button" disabled={submitting || !amount}>{submitting && <LoaderCircle className="spin" size={17} />}{t("modals.editTransaction.save")}</button>
        </div>
      </form>
    </ModalShell>
  );
}
