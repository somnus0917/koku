//! 编辑交易弹窗。
import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle, Plus, X } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import { TagEditor } from "./TagEditor";
import { CategoryAvatar } from "../../components/avatar";
import { formatMoney, toLocalDateTimeValue } from "../../lib";
import { clearTransactionSplits, listPayees, listTransactionSplits, setTransactionSplits } from "../../api";
import type { Account, Category, Payee, Tag, Transaction } from "../../types";

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
    payee_name?: string;
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
  const [payeeName, setPayeeName] = useState(transaction.payee_name ?? "");
  const [payees, setPayees] = useState<Payee[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  // 商户自动补全数据（datalist 客户端过滤）。
  useEffect(() => {
    let cancelled = false;
    listPayees()
      .then((items) => {
        if (!cancelled) setPayees(items);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  // 拆分分类：仅 expense/income 支持；加载现有拆分作为初始快照。
  const [splits, setSplits] = useState<{ category_id: number; amount: string; note: string }[]>([]);
  const [originalSplits, setOriginalSplits] = useState<{ category_id: number; amount: string; note: string }[]>([]);
  const [splitsLoaded, setSplitsLoaded] = useState(false);
  useEffect(() => {
    let cancelled = false;
    listTransactionSplits(transaction.id)
      .then((items) => {
        if (cancelled) return;
        const rows = items.map((item) => ({
          category_id: item.category_id,
          amount: item.amount,
          note: item.note ?? ""
        }));
        setSplits(rows);
        setOriginalSplits(rows);
      })
      .catch(() => undefined)
      .finally(() => {
        if (!cancelled) setSplitsLoaded(true);
      });
    return () => {
      cancelled = true;
    };
  }, [transaction.id]);
  const assigned = splits.reduce((sum, row) => sum + (Number(row.amount) || 0), 0);
  const totalAmount = Number(amount) || 0;
  const remaining = totalAmount - assigned;
  const splitsChanged = JSON.stringify(splits) !== JSON.stringify(originalSplits);
  const updateSplit = (index: number, patch: Partial<{ category_id: number; amount: string; note: string }>) => {
    setSplits((rows) => rows.map((row, i) => (i === index ? { ...row, ...patch } : row)));
  };
  const addSplit = () => {
    setSplits((rows) => [...rows, { category_id: matchingCategories[0]?.id ?? 0, amount: "", note: "" }]);
  };
  const removeSplit = (index: number) => {
    setSplits((rows) => rows.filter((_, i) => i !== index));
  };

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
        payee_name?: string;
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
      // 商户：值变化才提交；空串表示清除。
      if (payeeName.trim() !== (transaction.payee_name ?? "")) {
        input.payee_name = payeeName.trim();
      }
      if (splits.length > 0 && remaining.toFixed(2) !== "0.00") {
        setError(t("modals.editTransaction.splitRemainingError"));
        setSubmitting(false);
        return;
      }
      if (Object.keys(input).length === 0 && !splitsChanged) {
        onClose();
        return;
      }
      if (Object.keys(input).length > 0) {
        await onSubmit(input);
      }
      if (splitsChanged) {
        if (splits.length > 0) {
          await setTransactionSplits(transaction.id, splits);
        } else {
          await clearTransactionSplits(transaction.id);
        }
      }
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
          <label className="span-two"><span>{t("modals.transaction.payee")}</span>
            <input
              list="koku-payee-suggestions"
              value={payeeName}
              onChange={(e) => setPayeeName(e.target.value)}
              placeholder={t("modals.transaction.payeePlaceholder")}
            />
            <datalist id="koku-payee-suggestions">
              {payees.map((payee) => <option key={payee.id} value={payee.name} />)}
            </datalist>
          </label>
          <label className="span-two"><span>{t("common.tags")}</span>
            <TagEditor value={tagNames} onChange={setTagNames} suggestions={tags.map((tag) => tag.name)} />
          </label>
        </div>
        <div className="split-section">
          <div className="section-heading compact-heading">
            <div><span>SPLIT</span><h2>{t("modals.editTransaction.splits")}</h2></div>
          </div>
          {splits.map((row, index) => (
            <div className="split-row" key={index}>
              <select
                value={row.category_id}
                onChange={(e) => updateSplit(index, { category_id: Number(e.target.value) })}
              >
                {matchingCategories.map((item) => (
                  <option key={item.id} value={item.id}>{item.name}</option>
                ))}
              </select>
              <input
                type="number"
                min="0.01"
                step="0.01"
                inputMode="decimal"
                value={row.amount}
                onChange={(e) => updateSplit(index, { amount: e.target.value })}
                placeholder="0.00"
              />
              <input
                value={row.note}
                onChange={(e) => updateSplit(index, { note: e.target.value })}
                placeholder={t("common.optional")}
              />
              <button
                type="button"
                className="row-action"
                onClick={() => removeSplit(index)}
                aria-label={t("modals.editTransaction.removeSplit")}
              ><X size={15} /></button>
            </div>
          ))}
          <div className="split-actions">
            <button type="button" className="text-button" onClick={addSplit}><Plus size={15} /> {t("modals.editTransaction.addSplit")}</button>
          </div>
          {splits.length > 0 && (
            <div className="split-totals">
              <span>{t("modals.editTransaction.splitTotal", { amount: formatMoney(amount, transaction.currency) })}</span>
              <span>{t("modals.editTransaction.splitAssigned", { amount: formatMoney(assigned.toFixed(2), transaction.currency) })}</span>
              <span className={Math.abs(remaining) < 0.005 ? "positive" : "negative"}>
                {t("modals.editTransaction.splitRemaining", { amount: formatMoney(remaining.toFixed(2), transaction.currency) })}
              </span>
            </div>
          )}
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
