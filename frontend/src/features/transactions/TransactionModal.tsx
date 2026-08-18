//! 新建交易弹窗（支出/收入/转账，含外币折算与标签）。
import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import { RateHintLine, useRateHint } from "../../components/RateHint";
import { TagEditor } from "./TagEditor";
import { CategoryAvatar } from "../../components/avatar";
import { availableCurrencies, localDateTimeValue, readQuickEntry, writeQuickEntry } from "../../lib";
import { listPayees } from "../../api";
import type { createTransaction, createTransfer } from "../../api";
import type { Account, Category, Payee, Tag, TransactionKind } from "../../types";

export type TransactionSubmit =
  | { kind: "expense" | "income"; payload: Parameters<typeof createTransaction>[0] }
  | { kind: "transfer"; payload: Parameters<typeof createTransfer>[0] };

export function TransactionModal({
  accounts,
  categories,
  tags,
  onClose,
  onSubmit
}: {
  accounts: Account[];
  categories: Category[];
  tags: Tag[];
  onClose: () => void;
  onSubmit: (input: TransactionSubmit) => Promise<void>;
}) {
  const [lastEntry] = useState(() => readQuickEntry());
  const initialAccountId = lastEntry?.account_id ?? accounts[0]?.id ?? 0;
  const [kind, setKind] = useState<Exclude<TransactionKind, "loan" | "adjustment" | "trade" | "deposit">>(lastEntry?.kind ?? "expense");
  const [accountId, setAccountId] = useState(initialAccountId);
  const [targetId, setTargetId] = useState(accounts[1]?.id ?? accounts[0]?.id ?? 0);
  const [sourceCurrency, setSourceCurrency] = useState(accounts.find((account) => account.id === initialAccountId)?.currency ?? "CNY");
  const [categoryId, setCategoryId] = useState(lastEntry?.category_id ?? categories.find((item) => item.kind === "expense")?.id ?? 0);
  const [amount, setAmount] = useState("");
  const [settledAmount, setSettledAmount] = useState("");
  const [settledTouched, setSettledTouched] = useState(false);
  const [targetAmount, setTargetAmount] = useState("");
  const [note, setNote] = useState(lastEntry?.note ?? "");
  const [occurredAt, setOccurredAt] = useState(localDateTimeValue);
  const [tagNames, setTagNames] = useState<string[]>([]);
  const [payeeName, setPayeeName] = useState("");
  const [payees, setPayees] = useState<Payee[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
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
  const matchingCategories = categories.filter((item) => item.kind === kind);
  const selectedCategory = categories.find((item) => item.id === categoryId);
  const currencyOptions = availableCurrencies(accounts);
  const source = accounts.find((item) => item.id === accountId);
  const target = accounts.find((item) => item.id === targetId);
  const foreignTransaction = kind !== "transfer" && Boolean(source) && sourceCurrency !== source?.currency;
  const crossCurrency = kind === "transfer" && source?.currency !== target?.currency;
  const sameTransferEndpoint = kind === "transfer" && accountId === targetId;
  const { hint, status, refresh } = useRateHint(
    foreignTransaction ? sourceCurrency : null,
    foreignTransaction ? source?.currency ?? null : null
  );
  // 汇率就绪后用真实汇率预填「计入账户余额」（用户手动改过则不覆盖）。
  useEffect(() => {
    if (foreignTransaction && status === "ok" && hint && !settledTouched) {
      setSettledAmount((Number(amount) * Number(hint.rate)).toFixed(2));
    }
  }, [foreignTransaction, status, hint, amount, settledTouched]);

  const changeKind = (nextKind: Exclude<TransactionKind, "loan" | "adjustment" | "trade" | "deposit">) => {
    setKind(nextKind);
    if (nextKind !== "transfer") {
      setCategoryId(categories.find((item) => item.kind === nextKind)?.id ?? 0);
    }
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true);
    setFormError(null);
    try {
      const isoDate = new Date(occurredAt).toISOString();
      if (kind === "transfer") {
        await onSubmit({
          kind,
          payload: {
            from_account_id: accountId,
            to_account_id: targetId,
            source_amount: amount,
            target_amount: crossCurrency ? targetAmount : amount,
            occurred_at: isoDate,
            note
          }
        });
      } else {
        await onSubmit({
          kind,
          payload: {
            kind,
            account_id: accountId,
            category_id: categoryId,
            amount,
            currency: sourceCurrency,
            settled_amount: foreignTransaction ? settledAmount : amount,
            occurred_at: isoDate,
            note,
            tag_names: tagNames,
            payee_name: payeeName.trim() ? payeeName.trim() : undefined
          }
        });
        writeQuickEntry({ kind, account_id: accountId, category_id: categoryId, amount, note });
      }
    } catch (reason) {
      setFormError(reason instanceof Error ? reason.message : t("modals.transaction.saveFailed"));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <ModalShell eyebrow="NEW ENTRY" title={t("common.quickAdd")} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="kind-tabs">
          {(["expense", "income", "transfer"] as const).map((item) => (
            <button type="button" key={item} className={kind === item ? "active" : ""} onClick={() => changeKind(item)}>
              {t(`transactions.kind.${item}`)}
            </button>
          ))}
        </div>
        <label className="amount-field"><span>{t(kind === "income" ? "modals.transaction.incomeAmount" : kind === "transfer" ? "modals.transaction.transferAmount" : "modals.transaction.expenseAmount")} · {kind === "transfer" ? source?.currency : sourceCurrency}</span><div><em>{(kind === "transfer" ? source?.currency : sourceCurrency) === "CNY" ? "¥" : (kind === "transfer" ? source?.currency : sourceCurrency) === "USD" ? "$" : kind === "transfer" ? source?.currency : sourceCurrency}</em><input autoFocus required min="0.01" step="0.01" inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="0.00" /></div></label>
        <div className="form-grid">
          <label><span>{kind === "transfer" ? t("modals.transaction.fromAccount") : t("common.account")}</span><select value={accountId} onChange={(e) => setAccountId(Number(e.target.value))}>{accounts.map((item) => <option value={item.id} key={item.id}>{item.name} · {item.currency}</option>)}</select></label>
          {kind === "transfer" ? (
            <label><span>{t("modals.transaction.toAccount")}</span><select value={targetId} onChange={(e) => setTargetId(Number(e.target.value))}>{accounts.filter((item) => item.id !== accountId).map((item) => <option value={item.id} key={item.id}>{item.name} · {item.currency}</option>)}</select></label>
          ) : (
            <>
              <label><span>{t("modals.transaction.currency")}</span><select value={sourceCurrency} onChange={(e) => setSourceCurrency(e.target.value)}>{currencyOptions.map((item) => <option value={item} key={item}>{item}</option>)}</select></label>
              <label><span>{t("common.category")}</span><div className="category-input"><CategoryAvatar name={selectedCategory?.name ?? t("modals.transaction.defaultCategory")} size="small" /><select value={categoryId} onChange={(e) => setCategoryId(Number(e.target.value))}>{matchingCategories.map((item) => <option value={item.id} key={item.id}>{item.name}</option>)}</select></div></label>
              <label className="span-two"><span>{t("modals.transaction.payee")}</span>
                <input list="koku-payee-suggestions" value={payeeName} onChange={(e) => setPayeeName(e.target.value)} placeholder={t("modals.transaction.payeePlaceholder")} />
                <datalist id="koku-payee-suggestions">
                  {payees.map((payee) => <option key={payee.id} value={payee.name} />)}
                </datalist>
              </label>
            </>
          )}
          {foreignTransaction && (
            <>
              <label><span>{t("modals.transaction.settled", { currency: source?.currency })}</span><input required min="0.01" step="0.01" inputMode="decimal" value={settledAmount} onChange={(e) => { setSettledTouched(true); setSettledAmount(e.target.value); }} placeholder={t("modals.transaction.settledPlaceholder")} /></label>
              <RateHintLine from={sourceCurrency} to={source?.currency ?? ""} status={status} hint={hint} onRefresh={refresh} />
            </>
          )}
          {crossCurrency && <label><span>{t("modals.transaction.targetAmount", { currency: target?.currency })}</span><input required min="0.01" step="0.01" inputMode="decimal" value={targetAmount} onChange={(e) => setTargetAmount(e.target.value)} placeholder="0.00" /></label>}
          <label><span>{t("common.time")}</span><input required type="datetime-local" value={occurredAt} onChange={(e) => setOccurredAt(e.target.value)} /></label>
          <label className={kind === "transfer" && crossCurrency ? "" : "span-two"}><span>{t("common.note")}</span><input value={note} onChange={(e) => setNote(e.target.value)} placeholder={t("modals.transaction.notePlaceholder")} /></label>
          {kind !== "transfer" && (
            <label className="span-two"><span>{t("common.tags")}</span><TagEditor value={tagNames} onChange={setTagNames} suggestions={tags.map((tag) => tag.name)} /></label>
          )}
        </div>
        {sameTransferEndpoint && <div className="form-error">{t("modals.transaction.sameEndpointError")}</div>}
        {formError && <div className="form-error">{formError}</div>}
        <div className="modal-actions"><button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button><button className="primary-button" disabled={submitting || !amount || (kind !== "transfer" && !categoryId) || sameTransferEndpoint || (foreignTransaction && !settledAmount) || (crossCurrency && !targetAmount)}>{submitting && <LoaderCircle className="spin" size={17} />}{submitting ? t("common.saving") : t("modals.transaction.submit")}</button></div>
      </form>
    </ModalShell>
  );
}
