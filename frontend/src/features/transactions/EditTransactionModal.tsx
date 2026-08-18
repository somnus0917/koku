//! 编辑交易弹窗。
import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle, Plus, RefreshCcw, X } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import { TagEditor } from "./TagEditor";
import { CategoryAvatar } from "../../components/avatar";
import { formatMoney, toLocalDateTimeValue } from "../../lib";
import { listPayees, listTransactionSplits } from "../../api";
import { buildEditInput, type SplitRow, type TransactionEditInput } from "./editInput";
import type { Account, Category, Payee, Tag, Transaction } from "../../types";

/** 拆分加载状态：加载中 / 成功 / 失败（失败绝不静默当成「无拆分」）。 */
type SplitStatus = "loading" | "loaded" | "error";

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
  onSubmit: (input: TransactionEditInput) => Promise<void>;
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
  // 三态加载：加载中禁用保存；失败显示错误 + 重试并禁用编辑/保存，
  // 绝不在加载失败时把拆分当作「空」处理（否则保存会误删拆分）。
  const [splits, setSplits] = useState<SplitRow[]>([]);
  const [originalSplits, setOriginalSplits] = useState<SplitRow[]>([]);
  const [splitsStatus, setSplitsStatus] = useState<SplitStatus>("loading");
  const [splitsReloadKey, setSplitsReloadKey] = useState(0);
  useEffect(() => {
    let cancelled = false;
    setSplitsStatus("loading");
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
        setSplitsStatus("loaded");
      })
      .catch(() => {
        if (!cancelled) setSplitsStatus("error");
      });
    return () => {
      cancelled = true;
    };
  }, [transaction.id, splitsReloadKey]);
  const splitsReady = splitsStatus === "loaded";
  const assigned = splits.reduce((sum, row) => sum + (Number(row.amount) || 0), 0);
  const totalAmount = Number(amount) || 0;
  const remaining = totalAmount - assigned;
  const updateSplit = (index: number, patch: Partial<SplitRow>) => {
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
    // 拆分信息未就绪（加载中/失败）不可保存：按钮已禁用，这里双保险。
    if (!splitsReady) return;
    setSubmitting(true); setError(null);
    try {
      const input = buildEditInput({
        transaction,
        note,
        occurredAt,
        categoryId,
        amount,
        settledAmount,
        accountId,
        tagNames,
        payeeName,
        foreign,
        splits,
        originalSplits
      });
      // 前端合计校验仅作即时反馈（UX）；金额真值以后端 Decimal 为准。
      if (splits.length > 0 && Math.abs(remaining) >= 0.005) {
        setError(t("modals.editTransaction.splitRemainingError"));
        setSubmitting(false);
        return;
      }
      if (Object.keys(input).length === 0) {
        onClose();
        return;
      }
      // 父交易字段与拆分在同一个 PATCH 里原子提交，由父组件统一关闭/toast/刷新。
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
              <CategoryAvatar name={matchingCategories.find((item) => item.id === categoryId)?.name ?? t("modals.editTransaction.defaultCategory")} icon={matchingCategories.find((item) => item.id === categoryId)?.icon} size="small" />
              <select value={categoryId} onChange={(e) => setCategoryId(Number(e.target.value))}>
                {matchingCategories.map((item) => (
                  <option key={item.id} value={item.id}>{item.name}</option>
                ))}
              </select>
            </div>
          </label>
          {splitsReady && splits.length > 0 && (
            <p className="fx-hint span-two">{t("modals.editTransaction.splitCategoryHint")}</p>
          )}
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
          {splitsStatus === "loading" && (
            <p className="fx-hint">{t("modals.editTransaction.splitLoading")}</p>
          )}
          {splitsStatus === "error" && (
            <div className="split-error">
              <p className="fx-hint">{t("modals.editTransaction.splitLoadError")}</p>
              <button
                type="button"
                className="text-button"
                onClick={() => setSplitsReloadKey((key) => key + 1)}
              ><RefreshCcw size={15} /> {t("modals.editTransaction.splitRetry")}</button>
            </div>
          )}
          {splitsReady && (
            <>
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
            </>
          )}
        </div>
        {reimbursementLocked && <p className="fx-hint">{t("modals.editTransaction.lockedNote")}</p>}
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
          <button className="primary-button" disabled={submitting || !amount || !splitsReady}>{submitting && <LoaderCircle className="spin" size={17} />}{t("modals.editTransaction.save")}</button>
        </div>
      </form>
    </ModalShell>
  );
}
