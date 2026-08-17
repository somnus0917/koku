//! 弹窗组件：新建/编辑账户、交易、分类、定期、报销、借款、二步验证、对账。
import { useEffect, useState, type CSSProperties, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { uiLocale } from "../i18n";
import {
  BadgeDollarSign,
  Check,
  ChevronDown,
  CircleDollarSign,
  ClipboardCheck,
  Copy,
  KeyRound,
  LoaderCircle,
  LockKeyhole,
  PiggyBank,
  RefreshCcw,
  RotateCcw,
  ShieldCheck,
  Upload,
  X,
  type LucideIcon
} from "lucide-react";
import {
  cancelReconciliation,
  completeReconciliation,
  createReconciliation,
  getAuthSession,
  importTransactions,
  listReconciliations,
  rateHint,
  totpDisable,
  totpEnable,
  totpSetup
} from "../api";
import type { createTransaction, createTransfer } from "../api";
import {
  availableCurrencies,
  formatDate,
  formatMoney,
  localDateTimeValue,
  readQuickEntry,
  toLocalDateTimeValue,
  writeQuickEntry
} from "../lib";
import { CategoryAvatar } from "./avatar";
import type {
  Account,
  AccountType,
  Category,
  CategoryKind,
  Deposit,
  ImportResult,
  Loan,
  LoanType,
  RateQuote,
  Reconciliation,
  ReconciliationStatus,
  RecurrenceFrequency,
  Tag,
  Transaction,
  TransactionKind
} from "../types";

/** 参考汇率文本：如 1 USD ≈ 7.1445 CNY（2026-08-14）。 */
function formatRate(rate: string) {
  return Number(rate).toFixed(4).replace(/\.?0+$/, "");
}

/** 跨币种汇率提示：自动拉取 /api/rates（服务端带缓存与多源回退），失败时可手动重试。 */
function useRateHint(from: string | null, to: string | null) {
  const [hint, setHint] = useState<RateQuote | null>(null);
  const [status, setStatus] = useState<"idle" | "loading" | "ok" | "error">("idle");
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    if (!from || !to || from === to) {
      setHint(null);
      setStatus("idle");
      return;
    }
    let cancelled = false;
    setStatus("loading");
    rateHint(from, to)
      .then((quote) => {
        if (!cancelled) {
          setHint(quote);
          setStatus("ok");
        }
      })
      .catch(() => {
        if (!cancelled) {
          setHint(null);
          setStatus("error");
        }
      });
    return () => {
      cancelled = true;
    };
  }, [from, to, attempt]);
  const refresh = () => setAttempt((n) => n + 1);
  return { hint, status, refresh };
}

function RateHintLine({
  from,
  to,
  status,
  hint,
  onRefresh
}: {
  from: string;
  to: string;
  status: "idle" | "loading" | "ok" | "error";
  hint: RateQuote | null;
  onRefresh?: () => void;
}) {
  const { t } = useTranslation();
  if (status === "loading") {
    return <p className="fx-hint">{t("modals.rate.loading")}</p>;
  }
  if (status === "ok" && hint) {
    return (
      <p className="fx-hint">
        {t("modals.rate.hint", {
          from,
          rate: formatRate(hint.rate),
          to,
          meta: t("modals.rate.meta", { date: hint.date, source: hint.source, stale: hint.stale ? t("modals.rate.stale") : "" })
        })}
        {onRefresh && (
          <button type="button" className="fx-hint-refresh" onClick={onRefresh} title={t("modals.rate.refreshAria")} aria-label={t("modals.rate.refreshAria")}>
            <RefreshCcw size={11} />
          </button>
        )}
      </p>
    );
  }
  if (status === "error") {
    return (
      <p className="fx-hint error">
        {t("modals.rate.error")}
        {onRefresh && (
          <button type="button" className="fx-hint-refresh" onClick={onRefresh}>
            {t("modals.rate.retry")}
          </button>
        )}
      </p>
    );
  }
  return null;
}

/** 标签编辑：回车添加、点击 × 移除，附带已有标签建议。 */
export function TagEditor({
  value,
  onChange,
  suggestions
}: {
  value: string[];
  onChange: (tags: string[]) => void;
  suggestions: string[];
}) {
  const [draft, setDraft] = useState("");
  const { t } = useTranslation();
  const add = () => {
    const name = draft.trim();
    if (name && !value.includes(name)) onChange([...value, name]);
    setDraft("");
  };
  return (
    <div className="tag-editor">
      {value.map((name) => (
        <span className="tag-chip" key={name}>
          {name}
          <button type="button" onClick={() => onChange(value.filter((item) => item !== name))} aria-label={t("modals.tagEditor.removeAria", { name })}><X size={11} /></button>
        </span>
      ))}
      <input
        list="koku-tag-suggestions"
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            add();
          }
        }}
        onBlur={add}
        placeholder={t("modals.tagEditor.placeholder")}
      />
      <datalist id="koku-tag-suggestions">
        {suggestions.map((name) => <option key={name} value={name} />)}
      </datalist>
    </div>
  );
}

export function DepositModal({
  source,
  onClose,
  onSubmit
}: {
  source: Account;
  onClose: () => void;
  onSubmit: (input: { amount: string; rate: string; term_days: number; note?: string }) => Promise<void>;
}) {
  const [amount, setAmount] = useState("");
  const [rate, setRate] = useState("");
  const [termDays, setTermDays] = useState("90");
  const [note, setNote] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      await onSubmit({ amount, rate, term_days: Number(termDays), note: note || undefined });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("common.opFailed"));
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="FIXED DEPOSIT" title={t("modals.deposit.title")} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="deposit-info">
          <p>{t("modals.deposit.infoPrefix")}<strong>{source.name}</strong>{t("modals.deposit.infoSuffix", { currency: source.currency, balance: formatMoney(source.balance, source.currency) })}</p>
        </div>
        <div className="form-grid">
          <label><span>{t("modals.deposit.amount")}</span><input required autoFocus step="0.01" inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="0.00" /></label>
          <label><span>{t("modals.deposit.rate")}</span><input required step="0.01" inputMode="decimal" value={rate} onChange={(e) => setRate(e.target.value)} placeholder={t("modals.deposit.ratePlaceholder")} /></label>
          <label><span>{t("modals.deposit.termDays")}</span><input required type="number" min={1} value={termDays} onChange={(e) => setTermDays(e.target.value)} /></label>
          <label className="span-two"><span>{t("common.note")}</span><input value={note} onChange={(e) => setNote(e.target.value)} placeholder={t("common.optional")} /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
          <button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}{t("modals.deposit.submit")}</button>
        </div>
      </form>
    </ModalShell>
  );
}

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
  const remaining = Math.max(0, Number(expense.amount) - Number(expense.reimbursed_amount)).toFixed(2);
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
  onSubmit: (input: { loan_type: LoanType; counterparty: string; amount: string; account_id: number; note?: string; due_at?: string }) => Promise<void>;
}) {
  const [loanType, setLoanType] = useState<LoanType>("lend");
  const [counterparty, setCounterparty] = useState("");
  const [accountId, setAccountId] = useState("");
  const [amount, setAmount] = useState("");
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

export function ModalShell({ title, eyebrow, onClose, children }: { title: string; eyebrow: string; onClose: () => void; children: React.ReactNode }) {
  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section className="modal-card" role="dialog" aria-modal="true" aria-label={title}>
        <header><div><span>{eyebrow}</span><h2>{title}</h2></div><button className="icon-button" onClick={onClose}><X size={19} /></button></header>
        {children}
      </section>
    </div>
  );
}

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
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const { t } = useTranslation();
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
            tag_names: tagNames
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

export function EditAccountModal({
  account,
  currencies,
  onClose,
  onSubmit
}: {
  account: Account;
  currencies: string[];
  onClose: () => void;
  onSubmit: (input: { details: { name?: string; account_type?: AccountType; currency?: string; credit_limit?: string | null }; adjustment?: string }) => Promise<void>;
}) {
  const [name, setName] = useState(account.name);
  const [type, setType] = useState<AccountType>(account.account_type);
  const [currency, setCurrency] = useState(account.currency);
  const [creditLimit, setCreditLimit] = useState(account.credit_limit ?? "");
  const [adjustment, setAdjustment] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      const limitChanged = creditLimit.trim() !== (account.credit_limit ?? "");
      await onSubmit({
        details: {
          name: name.trim() !== account.name ? name.trim() : undefined,
          account_type: type !== account.account_type ? type : undefined,
          currency: currency !== account.currency ? currency : undefined,
          credit_limit: limitChanged ? (creditLimit.trim() ? creditLimit.trim() : null) : undefined
        },
        adjustment: adjustment ? adjustment : undefined
      });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("modals.transaction.saveFailed"));
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="EDIT ACCOUNT" title={t("accounts.editTitle")} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="deposit-info">
          <p>{t("modals.editAccount.info", { name: account.name, amount: formatMoney(account.balance, account.currency) })}</p>
        </div>
        <div className="form-grid">
          <label className="span-two"><span>{t("modals.editAccount.name")}</span><input value={name} onChange={(e) => setName(e.target.value)} /></label>
          <label><span>{t("modals.editAccount.type")}</span><select value={type} onChange={(e) => setType(e.target.value as AccountType)}>
            <option value="cash">{t("accounts.type.cash")}</option>
            <option value="savings">{t("accounts.type.savings")}</option>
            <option value="stock">{t("accounts.type.stock")}</option>
            <option value="credit">{t("accounts.type.credit")}</option>
          </select></label>
          <label><span>{t("modals.editAccount.currency")}</span><select value={currency} onChange={(e) => setCurrency(e.target.value)}>
            {currencies.map((item) => <option key={item} value={item}>{item}</option>)}
          </select></label>
          <label className="span-two"><span>{t("modals.editAccount.adjustment")}</span>
            <input step="0.01" inputMode="decimal" value={adjustment} onChange={(e) => setAdjustment(e.target.value)} placeholder="0.00" />
          </label>
          <label className="span-two"><span>{t("modals.editAccount.creditLimit")}</span>
            <input step="0.01" inputMode="decimal" value={creditLimit} onChange={(e) => setCreditLimit(e.target.value)} placeholder={t("modals.editAccount.creditLimitPlaceholder")} />
          </label>
        </div>
        <p className="category-delete-note">{t("modals.editAccount.adjustmentNote")}</p>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
          <button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}{t("common.save")}</button>
        </div>
      </form>
    </ModalShell>
  );
}

export function AccountModal({ currencies, onClose, onSubmit }: { currencies: string[]; onClose: () => void; onSubmit: (input: { name: string; account_type: AccountType; currency: string; opening_balance: string; credit_limit?: string }) => Promise<void> }) {
  const [name, setName] = useState("");
  const [type, setType] = useState<AccountType>("cash");
  const [currency, setCurrency] = useState("CNY");
  const [balance, setBalance] = useState("0");
  const [creditLimit, setCreditLimit] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const submit = async (event: FormEvent) => {
    event.preventDefault(); setSubmitting(true); setError(null);
    try {
      await onSubmit({
        name,
        account_type: type,
        currency,
        opening_balance: balance,
        credit_limit: creditLimit.trim() ? creditLimit.trim() : undefined
      });
    }
    catch (reason) { setError(reason instanceof Error ? reason.message : t("modals.transaction.saveFailed")); setSubmitting(false); }
  };
  return (
    <ModalShell eyebrow="NEW ACCOUNT" title={t("accounts.newAccount")} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="form-grid">
          <label className="span-two"><span>{t("modals.account.name")}</span><input autoFocus required value={name} onChange={(e) => setName(e.target.value)} placeholder={t("modals.account.namePlaceholder")} /></label>
          <label><span>{t("modals.editAccount.type")}</span><select value={type} onChange={(e) => setType(e.target.value as AccountType)}>
            <option value="cash">{t("accounts.type.cash")}</option>
            <option value="savings">{t("accounts.type.savings")}</option>
            <option value="stock">{t("accounts.type.stock")}</option>
            <option value="credit">{t("accounts.type.credit")}</option>
          </select></label>
          <label><span>{t("modals.account.currency")}</span>
            <select value={currency} onChange={(e) => setCurrency(e.target.value)}>
              {currencies.map((item) => <option key={item} value={item}>{item}</option>)}
            </select>
          </label>
          <label className="span-two"><span>{t("modals.account.openingBalance")}</span><input required step="0.01" inputMode="decimal" value={balance} onChange={(e) => setBalance(e.target.value)} /></label>
          {type === "credit" && (
            <label className="span-two"><span>{t("modals.account.creditLimit")}</span><input step="0.01" inputMode="decimal" value={creditLimit} onChange={(e) => setCreditLimit(e.target.value)} placeholder={t("modals.editAccount.creditLimitPlaceholder")} /></label>
          )}
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions"><button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button><button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}{t("modals.account.submit")}</button></div>
      </form>
    </ModalShell>
  );
}

export function CategoryModal({ categories, onClose, onSubmit, onDelete }: { categories: Category[]; onClose: () => void; onSubmit: (input: { name: string; kind: CategoryKind }) => Promise<void>; onDelete: (category: Category) => Promise<void> }) {
  const [name, setName] = useState("");
  const [kind, setKind] = useState<CategoryKind>("expense");
  const [submitting, setSubmitting] = useState(false);
  const [deletingId, setDeletingId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const submit = async (event: FormEvent) => {
    event.preventDefault(); setSubmitting(true); setError(null);
    try { await onSubmit({ name, kind }); }
    catch (reason) { setError(reason instanceof Error ? reason.message : t("modals.transaction.saveFailed")); setSubmitting(false); }
  };
  const remove = async (category: Category) => {
    if (!window.confirm(t("modals.category.confirmDelete", { name: category.name }))) return;
    setDeletingId(category.id); setError(null);
    try { await onDelete(category); }
    catch (reason) { setError(reason instanceof Error ? reason.message : t("modals.category.deleteFailed")); }
    finally { setDeletingId(null); }
  };
  return (
    <ModalShell eyebrow="CATEGORIES" title={t("modals.category.title")} onClose={onClose}>
      <div className="category-library">
        {([
          { kind: "expense" as const, label: t("modals.category.expenseGroup") },
          { kind: "income" as const, label: t("modals.category.incomeGroup") }
        ]).map((group) => {
          const items = categories.filter((item) => item.kind === group.kind);
          return (
            <section key={group.kind}>
              <header><strong>{group.label}</strong><small>{t("common.countItems", { count: items.length })}</small></header>
              <div className="category-chip-list">
                {items.map((item) => <span key={item.id} className={item.kind}><CategoryAvatar name={item.name} size="tiny" /><span>{item.name}</span><button type="button" onClick={() => void remove(item)} disabled={deletingId !== null} aria-label={t("modals.category.removeAria", { name: item.name })}>{deletingId === item.id ? <LoaderCircle className="spin" size={11} /> : <X size={11} />}</button></span>)}
              </div>
            </section>
          );
        })}
      </div>
      <form className="entry-form category-form" onSubmit={submit}>
        <div className="form-grid">
          <label><span>{t("modals.category.kind")}</span><select value={kind} onChange={(e) => setKind(e.target.value as CategoryKind)}><option value="expense">{t("common.expense")}</option><option value="income">{t("common.income")}</option></select></label>
          <label><span>{t("modals.category.name")}</span><input autoFocus required value={name} onChange={(e) => setName(e.target.value)} placeholder={t("modals.category.namePlaceholder")} /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions"><button type="button" className="secondary-button" onClick={onClose}>{t("modals.category.done")}</button><button className="primary-button" disabled={submitting || !name}>{submitting && <LoaderCircle className="spin" size={17} />}{t("modals.category.add")}</button></div>
      </form>
    </ModalShell>
  );
}

export function RecurringModal({
  accounts,
  categories,
  onClose,
  onSubmit
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
}) {
  const [kind, setKind] = useState<"expense" | "income">("expense");
  const [accountId, setAccountId] = useState(accounts[0]?.id ?? 0);
  const [categoryId, setCategoryId] = useState("");
  const [amount, setAmount] = useState("");
  const [note, setNote] = useState("");
  const [frequency, setFrequency] = useState<RecurrenceFrequency>("monthly");
  const [startDate, setStartDate] = useState("");
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
    <ModalShell eyebrow="RECURRING" title={t("modals.recurring.title")} onClose={onClose}>
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
          <button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}{t("common.create")}</button>
        </div>
      </form>
    </ModalShell>
  );
}

export function TradeModal({
  accounts,
  initialSide,
  initialSymbol,
  onClose,
  onSubmit
}: {
  accounts: Account[];
  initialSide: "buy" | "sell";
  initialSymbol: string;
  onClose: () => void;
  onSubmit: (input: {
    side: "buy" | "sell";
    payload: {
      account_id: number;
      symbol: string;
      shares: string;
      price: string;
      occurred_at?: string;
      note?: string;
    };
  }) => Promise<void>;
}) {
  const stockAccounts = accounts.filter((account) => account.account_type === "stock");
  const [side, setSide] = useState<"buy" | "sell">(initialSide);
  const [accountId, setAccountId] = useState(stockAccounts[0]?.id ?? 0);
  const [symbol, setSymbol] = useState(initialSymbol);
  const [shares, setShares] = useState("");
  const [price, setPrice] = useState("");
  const [note, setNote] = useState("");
  const [occurredAt, setOccurredAt] = useState(localDateTimeValue);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      await onSubmit({
        side,
        payload: {
          account_id: Number(accountId),
          symbol,
          shares,
          price,
          occurred_at: new Date(occurredAt).toISOString(),
          note: note || undefined
        }
      });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("common.opFailed"));
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="TRADE" title={t(side === "buy" ? "modals.trade.titleBuy" : "modals.trade.titleSell")} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="kind-tabs">
          {(["buy", "sell"] as const).map((item) => (
            <button type="button" key={item} className={side === item ? "active" : ""} onClick={() => setSide(item)}>
              {t(item === "buy" ? "modals.trade.buy" : "modals.trade.sell")}
            </button>
          ))}
        </div>
        <div className="form-grid">
          <label><span>{t("modals.trade.stockAccount")}</span>
            <select required value={accountId} onChange={(event) => setAccountId(Number(event.target.value))}>
              {stockAccounts.length === 0 && <option value={0} disabled>{t("modals.trade.noStockAccount")}</option>}
              {stockAccounts.map((account) => (
                <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>
              ))}
            </select>
          </label>
          <label><span>{t("modals.trade.symbol")}</span><input required autoFocus value={symbol} onChange={(event) => setSymbol(event.target.value)} placeholder={t("modals.trade.symbolPlaceholder")} /></label>
          <label><span>{t("modals.trade.shares")}</span><input required min="0.0001" step="0.0001" inputMode="decimal" value={shares} onChange={(event) => setShares(event.target.value)} placeholder="0" /></label>
          <label><span>{t("modals.trade.price")}</span><input required min="0.01" step="0.01" inputMode="decimal" value={price} onChange={(event) => setPrice(event.target.value)} placeholder="0.00" /></label>
          <label><span>{t("common.time")}</span><input type="datetime-local" value={occurredAt} onChange={(event) => setOccurredAt(event.target.value)} /></label>
          <label className="span-two"><span>{t("common.note")}</span><input value={note} onChange={(event) => setNote(event.target.value)} placeholder={t("common.optional")} /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
          <button className="primary-button" disabled={submitting || !symbol || !shares || !price || !accountId}>{submitting && <LoaderCircle className="spin" size={17} />}{t(side === "buy" ? "modals.trade.buy" : "modals.trade.sell")}</button>
        </div>
      </form>
    </ModalShell>
  );
}

export function PasswordModal({
  onClose,
  onSubmit
}: {
  onClose: () => void;
  onSubmit: (oldPassword: string, newPassword: string) => Promise<void>;
}) {  const [oldPassword, setOldPassword] = useState("");
  const [newPassword, setNewPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    if (newPassword.length < 8) {
      setError(t("modals.password.tooShort"));
      setSubmitting(false);
      return;
    }
    if (newPassword !== confirm) {
      setError(t("modals.password.mismatch"));
      setSubmitting(false);
      return;
    }
    try {
      await onSubmit(oldPassword, newPassword);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("modals.password.changeFailed"));
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="SECURITY" title={t("modals.password.title")} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="form-grid">
          <label className="span-two"><span>{t("modals.password.current")}</span><input required type="password" autoFocus autoComplete="current-password" value={oldPassword} onChange={(event) => setOldPassword(event.target.value)} /></label>
          <label className="span-two"><span>{t("modals.password.new")}</span><input required type="password" autoComplete="new-password" value={newPassword} onChange={(event) => setNewPassword(event.target.value)} /></label>
          <label className="span-two"><span>{t("modals.password.confirm")}</span><input required type="password" autoComplete="new-password" value={confirm} onChange={(event) => setConfirm(event.target.value)} /></label>
        </div>
        <p className="fx-hint">{t("modals.password.note")}</p>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
          <button className="primary-button" disabled={submitting || !oldPassword || !newPassword || !confirm}>{submitting && <LoaderCircle className="spin" size={17} />}{t("modals.password.submit")}</button>
        </div>
      </form>
    </ModalShell>
  );
}

/** 批量导入交易：选择账单文件与目标账户，导入后展示结果摘要（成功/重复/失败 + 问题行）。
 *  表单提交由本弹窗直接调用 API 以拿到 ImportResult 展示；「完成」时调用父级 onComplete
 *  （父级按 mutate 模式刷新并提示，不再重复导入）。 */
export function ImportModal({
  accounts,
  categories,
  onClose,
  onComplete
}: {
  accounts: Account[];
  categories: Category[];
  onClose: () => void;
  /** 导入已完成：仅刷新数据并提示，不再重复调用导入 API。 */
  onComplete: () => void;
}) {
  const [accountId, setAccountId] = useState("");
  const [format, setFormat] = useState<"auto" | "csv" | "qif" | "ofx">("auto");
  const [categoryId, setCategoryId] = useState("");
  const [currency, setCurrency] = useState("");
  const [file, setFile] = useState<File | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<ImportResult | null>(null);
  const { t } = useTranslation();

  const input = (): { format?: string; account_id: number; category_id?: number; currency?: string } => ({
    format: format === "auto" ? undefined : format,
    account_id: Number(accountId),
    category_id: categoryId ? Number(categoryId) : undefined,
    currency: currency.trim() || undefined
  });

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!file) return;
    setSubmitting(true); setError(null);
    try {
      setResult(await importTransactions(file, input()));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("modals.import.importFailed"));
      setSubmitting(false);
    }
  };

  const finish = () => {
    onComplete();
    onClose();
  };

  return (
    <ModalShell eyebrow="IMPORT" title={t("modals.import.title")} onClose={onClose}>
      {result ? (
        <div className="import-result">
          <div className="import-summary">
            <span className="import-count ok"><strong>{result.imported}</strong>{t("modals.import.imported", { count: result.imported })}</span>
            <span className="import-count skip"><strong>{result.skipped_duplicates}</strong>{t("modals.import.skipped", { count: result.skipped_duplicates })}</span>
            <span className={`import-count ${result.failed > 0 ? "bad" : ""}`}><strong>{result.failed}</strong>{t("modals.import.failed", { count: result.failed })}</span>
          </div>
          {result.issues.length > 0 && (
            <div className="import-issues" aria-label={t("modals.import.issuesAria")}>
              <div className="import-issues-head">{t("modals.import.issuesHead", { count: result.issues.length })}</div>
              {result.issues.map((issue, index) => (
                <div className="import-issue" key={index}>
                  <span>{t("modals.import.row", { line: issue.line })}</span>
                  <span>{issue.message}</span>
                </div>
              ))}
            </div>
          )}
          <p className="fx-hint">
            {t("modals.import.doneHint", { format: result.format.toUpperCase() })}
          </p>
          <div className="modal-actions">
            <button type="button" className="secondary-button" onClick={onClose}>{t("common.close")}</button>
            <button type="button" className="primary-button" onClick={finish}>{t("modals.category.done")}</button>
          </div>
        </div>
      ) : (
        <form className="entry-form" onSubmit={submit}>
          <div className="deposit-info">
            <p>{t("modals.import.intro")}</p>
          </div>
          <div className="form-grid">
            <label><span>{t("modals.import.account")}</span>
              <select required value={accountId} onChange={(e) => setAccountId(e.target.value)}>
                <option value="" disabled>{t("common.selectAccount")}</option>
                {accounts.map((account) => (
                  <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>
                ))}
              </select>
            </label>
            <label><span>{t("modals.import.format")}</span>
              <select value={format} onChange={(e) => setFormat(e.target.value as "auto" | "csv" | "qif" | "ofx")}>
                <option value="auto">{t("modals.import.auto")}</option>
                <option value="csv">CSV</option>
                <option value="qif">QIF</option>
                <option value="ofx">OFX</option>
              </select>
            </label>
            <label className="span-two"><span>{t("modals.import.defaultCategory")}</span>
              <select value={categoryId} onChange={(e) => setCategoryId(e.target.value)}>
                <option value="">{t("modals.import.noCategory")}</option>
                {categories.map((category) => (
                  <option key={category.id} value={category.id}>
                    {t(category.kind === "income" ? "modals.import.incomePrefix" : "modals.import.expensePrefix")}{category.name}
                  </option>
                ))}
              </select>
            </label>
            <label><span>{t("modals.import.defaultCurrency")}</span>
              <input value={currency} onChange={(e) => setCurrency(e.target.value)} placeholder={t("modals.import.currencyPlaceholder")} />
            </label>
            <label className="span-two"><span>{t("modals.import.file")}</span>
              <input
                required
                type="file"
                accept=".csv,.qif,.ofx"
                onChange={(e) => setFile(e.target.files?.[0] ?? null)}
              />
            </label>
          </div>
          {error && <div className="form-error">{error}</div>}
          <div className="modal-actions">
            <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
            <button className="primary-button" disabled={submitting || !file || !accountId}>
              {submitting ? <LoaderCircle className="spin" size={17} /> : <Upload size={16} />}
              {submitting ? t("modals.import.importing") : t("modals.import.start")}
            </button>
          </div>
        </form>
      )}
    </ModalShell>
  );
}

/** 当前本地日期 YYYY-MM-DD（对账日默认值）。 */
function todayDateValue(): string {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
}

/** 把 YYYY-MM-DD（或 RFC3339）解析为本地日期。 */
function parseDay(value: string): Date {
  return /^\d{4}-\d{2}-\d{2}$/.test(value) ? new Date(`${value}T00:00:00`) : new Date(value);
}

/** 日期展示（如 "2026年8月15日"），随界面语言变化。 */
function formatDay(value: string): string {
  return new Intl.DateTimeFormat(uiLocale(), { year: "numeric", month: "long", day: "numeric" }).format(parseDay(value));
}

/** 二步验证（TOTP）管理弹窗：查看状态、开始设置、关闭。 */
export function TotpModal({ onClose }: { onClose: () => void }) {
  const [loading, setLoading] = useState(true);
  const [enabled, setEnabled] = useState(false);
  const [step, setStep] = useState<"intro" | "password" | "secret" | "disable">("intro");
  const [secret, setSecret] = useState("");
  const [otpauthUri, setOtpauthUri] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [copied, setCopied] = useState<"" | "secret" | "uri">("");
  const { t } = useTranslation();

  useEffect(() => {
    let cancelled = false;
    getAuthSession()
      .then((session) => {
        if (!cancelled) {
          setEnabled(session.totp_enabled);
          setLoading(false);
        }
      })
      .catch((reason) => {
        if (!cancelled) {
          setError(reason instanceof Error ? reason.message : t("totp.loadFailed"));
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [t]);

  const copy = async (text: string, which: "secret" | "uri") => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(which);
      window.setTimeout(() => setCopied(""), 1600);
    } catch {
      setError(t("totp.copyFailed"));
    }
  };

  const startSetup = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true); setError(null);
    try {
      const setup = await totpSetup(password);
      setSecret(setup.secret);
      setOtpauthUri(setup.otpauth_uri);
      setPassword("");
      setStep("secret");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("totp.setupFailed"));
    } finally {
      setBusy(false);
    }
  };

  const enable = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true); setError(null);
    try {
      await totpEnable(code.trim());
      setCode("");
      setEnabled(true);
      setNotice(t("totp.enabledNotice"));
      setStep("intro");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("totp.enableFailed"));
    } finally {
      setBusy(false);
    }
  };

  const disable = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true); setError(null);
    try {
      await totpDisable(code.trim());
      setCode("");
      setEnabled(false);
      setNotice(t("totp.disabledNotice"));
      setStep("intro");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("totp.disableFailed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <ModalShell eyebrow="TWO-FACTOR AUTH" title={t("totp.title")} onClose={onClose}>
      <div className="entry-form">
        {loading ? (
          <div className="totp-loading"><LoaderCircle className="spin" size={18} /> {t("totp.loading")}</div>
        ) : step === "secret" ? (
          <>
            <p className="fx-hint">{t("totp.secretIntro")}</p>
            <div className="totp-secret-block">
              <span>{t("totp.secretLabel")}</span>
              <div className="totp-secret-row">
                <code className="totp-secret">{secret}</code>
                <button type="button" className="copy-button" onClick={() => void copy(secret, "secret")}>
                  {copied === "secret" ? <Check size={13} /> : <Copy size={13} />}
                  {copied === "secret" ? t("totp.copied") : t("totp.copy")}
                </button>
              </div>
            </div>
            <div className="totp-secret-block">
              <span>{t("totp.uriLabel")}</span>
              <div className="totp-secret-row">
                <code className="totp-uri">{otpauthUri}</code>
                <button type="button" className="copy-button" onClick={() => void copy(otpauthUri, "uri")}>
                  {copied === "uri" ? <Check size={13} /> : <Copy size={13} />}
                  {copied === "uri" ? t("totp.copied") : t("totp.copy")}
                </button>
              </div>
            </div>
            <form onSubmit={enable}>
              <div className="form-grid">
                <label className="span-two"><span>{t("totp.code")}</span>
                  <input required autoFocus inputMode="numeric" maxLength={6} pattern="[0-9]*" value={code} onChange={(e) => setCode(e.target.value)} placeholder={t("totp.codePlaceholder")} />
                </label>
              </div>
              {error && <div className="form-error">{error}</div>}
              <div className="modal-actions">
                <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
                <button className="primary-button" disabled={busy || code.trim().length !== 6}>
                  {busy && <LoaderCircle className="spin" size={17} />}{t("totp.confirmEnable")}
                </button>
              </div>
            </form>
          </>
        ) : step === "password" ? (
          <form onSubmit={startSetup}>
            <div className="deposit-info"><p>{t("totp.passwordIntro")}</p></div>
            <div className="form-grid">
              <label className="span-two"><span>{t("modals.password.current")}</span>
                <input required type="password" autoFocus autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} placeholder={t("totp.passwordPlaceholder")} />
              </label>
            </div>
            {error && <div className="form-error">{error}</div>}
            <div className="modal-actions">
              <button type="button" className="secondary-button" onClick={() => { setError(null); setStep("intro"); }}>{t("totp.back")}</button>
              <button className="primary-button" disabled={busy || !password}>{busy && <LoaderCircle className="spin" size={17} />}{t("totp.next")}</button>
            </div>
          </form>
        ) : enabled ? (
          <div className="totp-enabled">
            <p className="totp-status"><ShieldCheck size={17} /> {t("totp.enabledStatus")}</p>
            {notice && <div className="totp-notice" role="status"><Check size={14} /> {notice}</div>}
            {step === "disable" ? (
              <form onSubmit={disable}>
                <div className="deposit-info"><p>{t("totp.disableIntro")}</p></div>
                <div className="form-grid">
                  <label className="span-two"><span>{t("totp.currentCode")}</span>
                    <input required autoFocus inputMode="numeric" maxLength={6} pattern="[0-9]*" value={code} onChange={(e) => setCode(e.target.value)} placeholder={t("totp.codePlaceholder")} />
                  </label>
                </div>
                {error && <div className="form-error">{error}</div>}
                <div className="modal-actions">
                  <button type="button" className="secondary-button" onClick={() => { setError(null); setCode(""); setStep("intro"); }}>{t("common.cancel")}</button>
                  <button className="primary-button" disabled={busy || code.trim().length !== 6}>{busy && <LoaderCircle className="spin" size={17} />}{t("totp.disable")}</button>
                </div>
              </form>
            ) : (
              <>
                <p className="fx-hint">{t("totp.enabledHint")}</p>
                {error && <div className="form-error">{error}</div>}
                <div className="modal-actions">
                  <button type="button" className="secondary-button" onClick={onClose}>{t("common.close")}</button>
                  <button type="button" className="primary-button" onClick={() => { setError(null); setNotice(null); setStep("disable"); }}><KeyRound size={16} />{t("totp.disable")}</button>
                </div>
              </>
            )}
          </div>
        ) : (
          <>
            <p className="totp-intro-copy"><LockKeyhole size={17} /> {t("totp.disabledStatus")}</p>
            <p className="fx-hint">{t("totp.disabledHint")}</p>
            {notice && <div className="totp-notice" role="status"><Check size={14} /> {notice}</div>}
            {error && <div className="form-error">{error}</div>}
            <div className="modal-actions">
              <button type="button" className="secondary-button" onClick={onClose}>{t("common.close")}</button>
              <button type="button" className="primary-button" onClick={() => { setError(null); setNotice(null); setStep("password"); }}><ShieldCheck size={16} />{t("totp.startSetup")}</button>
            </div>
          </>
        )}
      </div>
    </ModalShell>
  );
}

function ReconciliationStatusBadge({ status }: { status: ReconciliationStatus }) {
  const { t } = useTranslation();
  const label = status === "open" ? t("reconcile.statusOpen") : status === "completed" ? t("reconcile.statusCompleted") : t("reconcile.statusCancelled");
  return <span className={`reconcile-status ${status}`}>{label}</span>;
}

/** 账户对账弹窗：查看对账历史、新建对账、完成/取消进行中的对账。 */
export function ReconciliationModal({
  account,
  onClose,
  onChanged
}: {
  account: Account;
  onClose: () => void;
  /** 完成对账后回调：父级刷新余额并提示。 */
  onChanged: () => void;
}) {
  const [items, setItems] = useState<Reconciliation[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [date, setDate] = useState(todayDateValue);
  const [balance, setBalance] = useState("");
  const [note, setNote] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [busyId, setBusyId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();

  const refresh = async () => {
    try {
      setItems(await listReconciliations(account.id));
      setLoadError(null);
    } catch (reason) {
      setLoadError(reason instanceof Error ? reason.message : t("reconcile.loadFailed"));
    }
  };
  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [account.id]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      await createReconciliation({
        account_id: account.id,
        statement_date: date,
        statement_balance: balance,
        note: note.trim() || undefined
      });
      setBalance("");
      setNote("");
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("reconcile.createFailed"));
    } finally {
      setSubmitting(false);
    }
  };

  const complete = async (item: Reconciliation) => {
    setBusyId(item.id); setError(null);
    try {
      await completeReconciliation(item.id);
      await onChanged();
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("reconcile.completeFailed"));
    } finally {
      setBusyId(null);
    }
  };

  const cancel = async (item: Reconciliation) => {
    setBusyId(item.id); setError(null);
    try {
      await cancelReconciliation(item.id);
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("reconcile.cancelFailed"));
    } finally {
      setBusyId(null);
    }
  };

  return (
    <ModalShell eyebrow="RECONCILE" title={t("reconcile.title", { name: account.name })} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="deposit-info">
          <p>{t("reconcile.intro", { amount: formatMoney(account.balance, account.currency) })}</p>
        </div>
        <div className="form-grid">
          <label><span>{t("reconcile.date")}</span><input required type="date" value={date} onChange={(e) => setDate(e.target.value)} /></label>
          <label><span>{t("reconcile.statementBalance", { currency: account.currency })}</span><input required step="0.01" inputMode="decimal" value={balance} onChange={(e) => setBalance(e.target.value)} placeholder="0.00" /></label>
          <label className="span-two"><span>{t("reconcile.note")}</span><input value={note} onChange={(e) => setNote(e.target.value)} placeholder={t("reconcile.notePlaceholder")} /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>{t("common.close")}</button>
          <button className="primary-button" disabled={submitting || !date || !balance}>
            {submitting && <LoaderCircle className="spin" size={17} />}{t("reconcile.create")}
          </button>
        </div>
      </form>

      <div className="reconcile-history">
        <div className="reconcile-history-head"><strong>{t("reconcile.history")}</strong><small>{items ? t("reconcile.count", { count: items.length }) : ""}</small></div>
        {loadError && <div className="form-error">{loadError}</div>}
        {items === null ? (
          loadError ? null : <div className="empty-hint"><LoaderCircle className="spin" size={16} /> {t("common.loading")}</div>
        ) : items.length === 0 ? (
          <div className="empty-hint">{t("reconcile.empty")}</div>
        ) : (
          <div className="reconcile-list">
            {items.map((item) => (
              <div className={`reconcile-item ${item.status}`} key={item.id}>
                <div className="reconcile-item-head">
                  <strong>{formatDay(item.statement_date)}</strong>
                  <ReconciliationStatusBadge status={item.status} />
                </div>
                <div className="reconcile-item-meta">
                  <span>{t("reconcile.statementLabel", { amount: formatMoney(item.statement_balance, account.currency) })}</span>
                  <span>{t("reconcile.bookLabel", { amount: formatMoney(item.book_balance, account.currency) })}</span>
                  <span>{t("reconcile.openedAt", { date: formatDate(item.opened_at) })}</span>
                </div>
                {item.note && <p className="fx-hint">{item.note}</p>}
                {item.completed_at && <p className="fx-hint">{t("reconcile.completedAt", { date: formatDate(item.completed_at) })}</p>}
                {item.adjustment_transaction_id != null && (
                  <p className="reconcile-adjustment"><RotateCcw size={12} /> {t("reconcile.adjustmentNote")}</p>
                )}
                {item.status === "open" && (
                  <div className="reconcile-actions">
                    <button type="button" className="text-button" disabled={busyId === item.id} onClick={() => void complete(item)}>
                      {busyId === item.id ? <LoaderCircle className="spin" size={13} /> : <ClipboardCheck size={13} />}{t("reconcile.complete")}
                    </button>
                    <button type="button" className="text-button danger" disabled={busyId === item.id} onClick={() => void cancel(item)}>
                      <X size={13} />{t("common.cancel")}
                    </button>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </ModalShell>
  );
}
