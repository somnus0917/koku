//! 弹窗组件：新建/编辑账户、交易、分类、定期、报销、借款、二步验证、对账。
import { useEffect, useState, type CSSProperties, type FormEvent } from "react";
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
  if (status === "loading") {
    return <p className="fx-hint">正在获取汇率…</p>;
  }
  if (status === "ok" && hint) {
    return (
      <p className="fx-hint">
        参考汇率 1 {from} ≈ {formatRate(hint.rate)} {to}（{hint.date}
        {hint.stale ? "，缓存" : ""}，{hint.source}），可修改
        {onRefresh && (
          <button type="button" className="fx-hint-refresh" onClick={onRefresh} title="重新获取汇率" aria-label="重新获取汇率">
            <RefreshCcw size={11} />
          </button>
        )}
      </p>
    );
  }
  if (status === "error") {
    return (
      <p className="fx-hint error">
        未能获取汇率，请手动填写
        {onRefresh && (
          <button type="button" className="fx-hint-refresh" onClick={onRefresh}>
            重新获取
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
          <button type="button" onClick={() => onChange(value.filter((item) => item !== name))} aria-label={`移除${name}`}><X size={11} /></button>
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
        placeholder="添加标签"
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
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      await onSubmit({ amount, rate, term_days: Number(termDays), note: note || undefined });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "操作失败");
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="FIXED DEPOSIT" title="转入定期" onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="deposit-info">
          <p>从 <strong>{source.name}</strong>（{source.currency} 可用 {formatMoney(source.balance, source.currency)}）转存一笔定期。</p>
        </div>
        <div className="form-grid">
          <label><span>转存金额</span><input required autoFocus step="0.01" inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="0.00" /></label>
          <label><span>年利率 (%)</span><input required step="0.01" inputMode="decimal" value={rate} onChange={(e) => setRate(e.target.value)} placeholder="例如 2.10" /></label>
          <label><span>期限（天）</span><input required type="number" min={1} value={termDays} onChange={(e) => setTermDays(e.target.value)} /></label>
          <label className="span-two"><span>备注</span><input value={note} onChange={(e) => setNote(e.target.value)} placeholder="可选" /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>取消</button>
          <button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}存入定期</button>
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
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      await onSubmit(Number(targetId));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "操作失败");
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="MATURE DEPOSIT" title="结清定期" onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="deposit-info">
          <p><strong>定期 · {deposit.term_days} 天</strong> · 年利率 {deposit.rate}%{deposit.maturity_at ? ` · ${formatDate(deposit.maturity_at)} 到期` : ""}</p>
          <p>当前本金 {formatMoney(deposit.amount, deposit.currency)}，结清时按实际持有天数计息，本息一并转回。</p>
        </div>
        <div className="form-grid">
          <label className="span-two"><span>转回账户</span>
            <select required value={targetId} onChange={(e) => setTargetId(e.target.value)}>
              <option value="" disabled>选择目标账户</option>
              {accounts.map((account) => (
                <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>
              ))}
            </select>
          </label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>取消</button>
          <button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}结清并转回</button>
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
      setError(reason instanceof Error ? reason.message : "操作失败");
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="REIMBURSEMENT" title="报销支出" onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="deposit-info">
          <p>{expense.note || "一笔支出"} · {formatMoney(expense.amount, expense.currency)}，剩余可报销 {formatMoney(remaining, expense.currency)}。</p>
        </div>
        <div className="form-grid">
          <label><span>报销到账账户</span>
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
              <option value="" disabled>选择账户</option>
              {accounts.map((account) => (
                <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>
              ))}
            </select>
          </label>
          <label><span>报销金额（{expense.currency}）</span><input required step="0.01" inputMode="decimal" max={remaining} value={amount} onChange={(e) => setAmount(e.target.value)} /></label>
          {crossCurrency && (
            <>
              <label className="span-two"><span>入账金额（{selectedAccount.currency}）</span>
                <input
                  required
                  step="0.01"
                  inputMode="decimal"
                  value={settledAmount}
                  onChange={(e) => {
                    setSettledTouched(true);
                    setSettledAmount(e.target.value);
                  }}
                  placeholder={`按汇率折算成 ${selectedAccount.currency}`}
                />
              </label>
              <div className="span-two">
                <RateHintLine from={expense.currency} to={selectedAccount.currency} status={status} hint={hint} onRefresh={refresh} />
              </div>
            </>
          )}
          <label className="span-two"><span>备注</span><input value={note} onChange={(e) => setNote(e.target.value)} placeholder="可选" /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>取消</button>
          <button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}确认报销</button>
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
      setError(reason instanceof Error ? reason.message : "操作失败");
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="LOAN" title="记一笔借款" onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="form-grid">
          <label><span>方向</span>
            <select value={loanType} onChange={(e) => setLoanType(e.target.value as LoanType)}>
              <option value="lend">借出（我借给别人）</option>
              <option value="borrow">借入（我向别人借）</option>
            </select>
          </label>
          <label><span>往来人</span>
            <input required autoFocus list="koku-counterparties" value={counterparty} onChange={(e) => setCounterparty(e.target.value)} placeholder="例如：张三" />
            <datalist id="koku-counterparties">
              {counterparties.map((name) => <option key={name} value={name} />)}
            </datalist>
          </label>
          <label><span>金额</span><input required step="0.01" inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="0.00" /></label>
          <label><span>资金账户</span>
            <select required value={accountId} onChange={(e) => setAccountId(e.target.value)}>
              <option value="" disabled>选择账户</option>
              {accounts.map((account) => (
                <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>
              ))}
            </select>
          </label>
          <label className="span-two"><span>备注</span><input value={note} onChange={(e) => setNote(e.target.value)} placeholder="可选" /></label>
          <label><span>到期日（可选）</span><input type="date" value={dueAt} onChange={(e) => setDueAt(e.target.value)} /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>取消</button>
          <button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}确认{loanType === "lend" ? "借出" : "借入"}</button>
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
      setError(reason instanceof Error ? reason.message : "操作失败");
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="REPAYMENT" title={`${loan.loan_type === "lend" ? "收回" : "偿还"}借款`} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="deposit-info">
          <p>{loan.loan_type === "lend" ? "借出" : "借入"}给 {loan.counterparty}，未结 {formatMoney(loan.outstanding, loan.currency)}。</p>
        </div>
        <div className="form-grid">
          <label><span>资金账户</span>
            <select required value={accountId} onChange={(e) => setAccountId(e.target.value)}>
              <option value="" disabled>选择账户</option>
              {accounts.map((account) => (
                <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>
              ))}
            </select>
          </label>
          <label><span>还款金额（{loan.currency}）</span><input required step="0.01" inputMode="decimal" max={loan.outstanding} value={amount} onChange={(e) => setAmount(e.target.value)} /></label>
          {crossCurrency && (
            <>
              <label className="span-two"><span>计入账户余额 · {selectedAccount.currency}</span>
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
                  placeholder={`按汇率折算成 ${selectedAccount.currency}`}
                />
              </label>
              <div className="span-two">
                <RateHintLine from={loan.currency} to={selectedAccount.currency} status={status} hint={hint} onRefresh={refresh} />
              </div>
            </>
          )}
          <label className="span-two"><span>备注</span><input value={note} onChange={(e) => setNote(e.target.value)} placeholder="可选" /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>取消</button>
          <button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}确认还款</button>
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
      setFormError(reason instanceof Error ? reason.message : "保存失败");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <ModalShell eyebrow="NEW ENTRY" title="记一笔" onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="kind-tabs">
          {(["expense", "income", "transfer"] as const).map((item) => (
            <button type="button" key={item} className={kind === item ? "active" : ""} onClick={() => changeKind(item)}>
              {item === "expense" ? "支出" : item === "income" ? "收入" : "转账"}
            </button>
          ))}
        </div>
        <label className="amount-field"><span>{kind === "income" ? "收入金额" : kind === "transfer" ? "转出金额" : "支出金额"} · {kind === "transfer" ? source?.currency : sourceCurrency}</span><div><em>{(kind === "transfer" ? source?.currency : sourceCurrency) === "CNY" ? "¥" : (kind === "transfer" ? source?.currency : sourceCurrency) === "USD" ? "$" : kind === "transfer" ? source?.currency : sourceCurrency}</em><input autoFocus required min="0.01" step="0.01" inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="0.00" /></div></label>
        <div className="form-grid">
          <label><span>{kind === "transfer" ? "转出账户" : "账户"}</span><select value={accountId} onChange={(e) => setAccountId(Number(e.target.value))}>{accounts.map((item) => <option value={item.id} key={item.id}>{item.name} · {item.currency}</option>)}</select></label>
          {kind === "transfer" ? (
            <label><span>转入账户</span><select value={targetId} onChange={(e) => setTargetId(Number(e.target.value))}>{accounts.filter((item) => item.id !== accountId).map((item) => <option value={item.id} key={item.id}>{item.name} · {item.currency}</option>)}</select></label>
          ) : (
            <>
              <label><span>交易币种</span><select value={sourceCurrency} onChange={(e) => setSourceCurrency(e.target.value)}>{currencyOptions.map((item) => <option value={item} key={item}>{item}</option>)}</select></label>
              <label><span>分类</span><div className="category-input"><CategoryAvatar name={selectedCategory?.name ?? "其他支出"} size="small" /><select value={categoryId} onChange={(e) => setCategoryId(Number(e.target.value))}>{matchingCategories.map((item) => <option value={item.id} key={item.id}>{item.name}</option>)}</select></div></label>
            </>
          )}
          {foreignTransaction && (
            <>
              <label><span>计入账户余额 · {source?.currency}</span><input required min="0.01" step="0.01" inputMode="decimal" value={settledAmount} onChange={(e) => { setSettledTouched(true); setSettledAmount(e.target.value); }} placeholder="换算后的结算金额" /></label>
              <RateHintLine from={sourceCurrency} to={source?.currency ?? ""} status={status} hint={hint} onRefresh={refresh} />
            </>
          )}
          {crossCurrency && <label><span>转入金额 · {target?.currency}</span><input required min="0.01" step="0.01" inputMode="decimal" value={targetAmount} onChange={(e) => setTargetAmount(e.target.value)} placeholder="0.00" /></label>}
          <label><span>时间</span><input required type="datetime-local" value={occurredAt} onChange={(e) => setOccurredAt(e.target.value)} /></label>
          <label className={kind === "transfer" && crossCurrency ? "" : "span-two"}><span>备注</span><input value={note} onChange={(e) => setNote(e.target.value)} placeholder="这笔钱花在了哪里？" /></label>
          {kind !== "transfer" && (
            <label className="span-two"><span>标签</span><TagEditor value={tagNames} onChange={setTagNames} suggestions={tags.map((tag) => tag.name)} /></label>
          )}
        </div>
        {sameTransferEndpoint && <div className="form-error">转出与转入账户不能相同。</div>}
        {formError && <div className="form-error">{formError}</div>}
        <div className="modal-actions"><button type="button" className="secondary-button" onClick={onClose}>取消</button><button className="primary-button" disabled={submitting || !amount || (kind !== "transfer" && !categoryId) || sameTransferEndpoint || (foreignTransaction && !settledAmount) || (crossCurrency && !targetAmount)}>{submitting && <LoaderCircle className="spin" size={17} />}{submitting ? "保存中" : "确认记录"}</button></div>
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
      setError(reason instanceof Error ? reason.message : "保存失败");
      setSubmitting(false);
    }
  };

  return (
    <ModalShell eyebrow="EDIT ENTRY" title="编辑交易" onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="deposit-info">
          <p>
            {transaction.kind === "expense" ? "支出" : "收入"} · {formatMoney(transaction.amount, transaction.currency)}
            {foreign ? `，结算 ${formatMoney(transaction.settled_amount, accountCurrency)}` : ""}
          </p>
        </div>
        <div className="form-grid">
          <label><span>账户</span>
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
          <label><span>分类</span>
            <div className="category-input">
              <CategoryAvatar name={matchingCategories.find((item) => item.id === categoryId)?.name ?? "其他"} size="small" />
              <select value={categoryId} onChange={(e) => setCategoryId(Number(e.target.value))}>
                {matchingCategories.map((item) => (
                  <option key={item.id} value={item.id}>{item.name}</option>
                ))}
              </select>
            </div>
          </label>
          <label><span>金额（{transaction.currency}）</span>
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
            <label><span>计入账户余额 · {accountCurrency}</span>
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
          <label><span>时间</span>
            <input required type="datetime-local" value={occurredAt} onChange={(e) => setOccurredAt(e.target.value)} />
          </label>
          <label className="span-two"><span>备注</span>
            <input value={note} onChange={(e) => setNote(e.target.value)} placeholder="可选" />
          </label>
          <label className="span-two"><span>标签</span>
            <TagEditor value={tagNames} onChange={setTagNames} suggestions={tags.map((tag) => tag.name)} />
          </label>
        </div>
        {reimbursementLocked && <p className="fx-hint">该笔支出已发生报销，仅可修改备注、分类和时间。</p>}
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>取消</button>
          <button className="primary-button" disabled={submitting || !amount}>{submitting && <LoaderCircle className="spin" size={17} />}保存修改</button>
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
      setError(reason instanceof Error ? reason.message : "保存失败");
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="EDIT ACCOUNT" title="编辑账户" onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="deposit-info">
          <p><strong>{account.name}</strong> · 当前余额 {formatMoney(account.balance, account.currency)}</p>
        </div>
        <div className="form-grid">
          <label className="span-two"><span>账户名称</span><input value={name} onChange={(e) => setName(e.target.value)} /></label>
          <label><span>账户类型</span><select value={type} onChange={(e) => setType(e.target.value as AccountType)}>
            <option value="cash">零钱</option>
            <option value="savings">储蓄</option>
            <option value="stock">股票</option>
            <option value="credit">信用</option>
          </select></label>
          <label><span>结算币种</span><select value={currency} onChange={(e) => setCurrency(e.target.value)}>
            {currencies.map((item) => <option key={item} value={item}>{item}</option>)}
          </select></label>
          <label className="span-two"><span>余额调整（正数增加 / 负数减少，留空则不调整）</span>
            <input step="0.01" inputMode="decimal" value={adjustment} onChange={(e) => setAdjustment(e.target.value)} placeholder="0.00" />
          </label>
          <label className="span-two"><span>信用额度（仅信用账户，留空表示不设置）</span>
            <input step="0.01" inputMode="decimal" value={creditLimit} onChange={(e) => setCreditLimit(e.target.value)} placeholder="例如 20000" />
          </label>
        </div>
        <p className="category-delete-note">余额调整会生成一条“余额调整”流水用于追溯，可在交易列表撤销。</p>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>取消</button>
          <button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}保存</button>
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
    catch (reason) { setError(reason instanceof Error ? reason.message : "保存失败"); setSubmitting(false); }
  };
  return (
    <ModalShell eyebrow="NEW ACCOUNT" title="新建账户" onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="form-grid">
          <label className="span-two"><span>账户名称</span><input autoFocus required value={name} onChange={(e) => setName(e.target.value)} placeholder="例如：储蓄卡" /></label>
          <label><span>账户类型</span><select value={type} onChange={(e) => setType(e.target.value as AccountType)}>
            <option value="cash">零钱</option>
            <option value="savings">储蓄</option>
            <option value="stock">股票</option>
            <option value="credit">信用</option>
          </select></label>
          <label><span>账户结算币种</span>
            <select value={currency} onChange={(e) => setCurrency(e.target.value)}>
              {currencies.map((item) => <option key={item} value={item}>{item}</option>)}
            </select>
          </label>
          <label className="span-two"><span>期初余额</span><input required step="0.01" inputMode="decimal" value={balance} onChange={(e) => setBalance(e.target.value)} /></label>
          {type === "credit" && (
            <label className="span-two"><span>信用额度（可选）</span><input step="0.01" inputMode="decimal" value={creditLimit} onChange={(e) => setCreditLimit(e.target.value)} placeholder="例如 20000" /></label>
          )}
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions"><button type="button" className="secondary-button" onClick={onClose}>取消</button><button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}创建账户</button></div>
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
  const submit = async (event: FormEvent) => {
    event.preventDefault(); setSubmitting(true); setError(null);
    try { await onSubmit({ name, kind }); }
    catch (reason) { setError(reason instanceof Error ? reason.message : "保存失败"); setSubmitting(false); }
  };
  const remove = async (category: Category) => {
    if (!window.confirm(`删除“${category.name}”？历史账单和统计不会受到影响。`)) return;
    setDeletingId(category.id); setError(null);
    try { await onDelete(category); }
    catch (reason) { setError(reason instanceof Error ? reason.message : "删除失败"); }
    finally { setDeletingId(null); }
  };
  return (
    <ModalShell eyebrow="CATEGORIES" title="管理分类" onClose={onClose}>
      <div className="category-library">
        {([
          { kind: "expense" as const, label: "支出分类" },
          { kind: "income" as const, label: "收入分类" }
        ]).map((group) => {
          const items = categories.filter((item) => item.kind === group.kind);
          return (
            <section key={group.kind}>
              <header><strong>{group.label}</strong><small>{items.length} 项</small></header>
              <div className="category-chip-list">
                {items.map((item) => <span key={item.id} className={item.kind}><CategoryAvatar name={item.name} size="tiny" /><span>{item.name}</span><button type="button" onClick={() => void remove(item)} disabled={deletingId !== null} aria-label={`删除${item.name}`}>{deletingId === item.id ? <LoaderCircle className="spin" size={11} /> : <X size={11} />}</button></span>)}
              </div>
            </section>
          );
        })}
      </div>
      <form className="entry-form category-form" onSubmit={submit}>
        <div className="form-grid">
          <label><span>分类类型</span><select value={kind} onChange={(e) => setKind(e.target.value as CategoryKind)}><option value="expense">支出</option><option value="income">收入</option></select></label>
          <label><span>新分类名称</span><input autoFocus required value={name} onChange={(e) => setName(e.target.value)} placeholder="例如：旅行" /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions"><button type="button" className="secondary-button" onClick={onClose}>完成</button><button className="primary-button" disabled={submitting || !name}>{submitting && <LoaderCircle className="spin" size={17} />}添加分类</button></div>
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
      setError(reason instanceof Error ? reason.message : "操作失败");
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="RECURRING" title="新建周期交易" onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="form-grid">
          <label><span>类型</span>
            <select value={kind} onChange={(event) => { setKind(event.target.value as "expense" | "income"); setCategoryId(""); }}>
              <option value="expense">支出</option>
              <option value="income">收入</option>
            </select>
          </label>
          <label><span>金额</span><input required step="0.01" inputMode="decimal" value={amount} onChange={(event) => setAmount(event.target.value)} placeholder="0.00" /></label>
          <label><span>资金账户</span>
            <select required value={accountId} onChange={(event) => setAccountId(Number(event.target.value))}>
              {accounts.map((account) => (
                <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>
              ))}
            </select>
          </label>
          <label><span>分类</span>
            <select required value={categoryId} onChange={(event) => setCategoryId(event.target.value)}>
              <option value="" disabled>选择分类</option>
              {kindCategories.map((category) => (
                <option key={category.id} value={category.id}>{category.name}</option>
              ))}
            </select>
          </label>
          <label><span>周期</span>
            <select value={frequency} onChange={(event) => setFrequency(event.target.value as RecurrenceFrequency)}>
              <option value="monthly">每月</option>
              <option value="weekly">每周</option>
            </select>
          </label>
          <label><span>首次日期</span><input required type="date" value={startDate} onChange={(event) => setStartDate(event.target.value)} /></label>
          <label className="span-two"><span>备注</span><input value={note} onChange={(event) => setNote(event.target.value)} placeholder="可选，例如：房租 / 视频会员" /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>取消</button>
          <button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}创建</button>
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
      setError(reason instanceof Error ? reason.message : "操作失败");
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="TRADE" title={side === "buy" ? "买入股票" : "卖出股票"} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="kind-tabs">
          {(["buy", "sell"] as const).map((item) => (
            <button type="button" key={item} className={side === item ? "active" : ""} onClick={() => setSide(item)}>
              {item === "buy" ? "买入" : "卖出"}
            </button>
          ))}
        </div>
        <div className="form-grid">
          <label><span>股票账户</span>
            <select required value={accountId} onChange={(event) => setAccountId(Number(event.target.value))}>
              {stockAccounts.length === 0 && <option value={0} disabled>没有股票账户</option>}
              {stockAccounts.map((account) => (
                <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>
              ))}
            </select>
          </label>
          <label><span>代码</span><input required autoFocus value={symbol} onChange={(event) => setSymbol(event.target.value)} placeholder="例如 AAPL" /></label>
          <label><span>股数</span><input required min="0.0001" step="0.0001" inputMode="decimal" value={shares} onChange={(event) => setShares(event.target.value)} placeholder="0" /></label>
          <label><span>每股价格</span><input required min="0.01" step="0.01" inputMode="decimal" value={price} onChange={(event) => setPrice(event.target.value)} placeholder="0.00" /></label>
          <label><span>时间</span><input type="datetime-local" value={occurredAt} onChange={(event) => setOccurredAt(event.target.value)} /></label>
          <label className="span-two"><span>备注</span><input value={note} onChange={(event) => setNote(event.target.value)} placeholder="可选" /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>取消</button>
          <button className="primary-button" disabled={submitting || !symbol || !shares || !price || !accountId}>{submitting && <LoaderCircle className="spin" size={17} />}{side === "buy" ? "买入" : "卖出"}</button>
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
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    if (newPassword.length < 8) {
      setError("新密码至少 8 位");
      setSubmitting(false);
      return;
    }
    if (newPassword !== confirm) {
      setError("两次输入的新密码不一致");
      setSubmitting(false);
      return;
    }
    try {
      await onSubmit(oldPassword, newPassword);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "修改失败");
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="SECURITY" title="修改密码" onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="form-grid">
          <label className="span-two"><span>当前密码</span><input required type="password" autoFocus autoComplete="current-password" value={oldPassword} onChange={(event) => setOldPassword(event.target.value)} /></label>
          <label className="span-two"><span>新密码（至少 8 位）</span><input required type="password" autoComplete="new-password" value={newPassword} onChange={(event) => setNewPassword(event.target.value)} /></label>
          <label className="span-two"><span>确认新密码</span><input required type="password" autoComplete="new-password" value={confirm} onChange={(event) => setConfirm(event.target.value)} /></label>
        </div>
        <p className="fx-hint">修改后所有登录会话将失效，需要重新登录。</p>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>取消</button>
          <button className="primary-button" disabled={submitting || !oldPassword || !newPassword || !confirm}>{submitting && <LoaderCircle className="spin" size={17} />}保存并重新登录</button>
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
      setError(reason instanceof Error ? reason.message : "导入失败");
      setSubmitting(false);
    }
  };

  const finish = () => {
    onComplete();
    onClose();
  };

  return (
    <ModalShell eyebrow="IMPORT" title="导入交易" onClose={onClose}>
      {result ? (
        <div className="import-result">
          <div className="import-summary">
            <span className="import-count ok"><strong>{result.imported}</strong>成功导入</span>
            <span className="import-count skip"><strong>{result.skipped_duplicates}</strong>重复跳过</span>
            <span className={`import-count ${result.failed > 0 ? "bad" : ""}`}><strong>{result.failed}</strong>失败</span>
          </div>
          {result.issues.length > 0 && (
            <div className="import-issues" aria-label="导入问题明细">
              <div className="import-issues-head">以下 {result.issues.length} 行被跳过或失败：</div>
              {result.issues.map((issue, index) => (
                <div className="import-issue" key={index}>
                  <span>第 {issue.line} 行</span>
                  <span>{issue.message}</span>
                </div>
              ))}
            </div>
          )}
          <p className="fx-hint">
            已导入 {result.format.toUpperCase()} 到该账户。点击「完成」刷新账本并提示结果。
          </p>
          <div className="modal-actions">
            <button type="button" className="secondary-button" onClick={onClose}>关闭</button>
            <button type="button" className="primary-button" onClick={finish}>完成</button>
          </div>
        </div>
      ) : (
        <form className="entry-form" onSubmit={submit}>
          <div className="deposit-info">
            <p>从账单文件批量导入流水，支持 CSV / QIF / OFX；格式可留空自动识别。</p>
          </div>
          <div className="form-grid">
            <label><span>目标账户</span>
              <select required value={accountId} onChange={(e) => setAccountId(e.target.value)}>
                <option value="" disabled>选择账户</option>
                {accounts.map((account) => (
                  <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>
                ))}
              </select>
            </label>
            <label><span>文件格式</span>
              <select value={format} onChange={(e) => setFormat(e.target.value as "auto" | "csv" | "qif" | "ofx")}>
                <option value="auto">自动识别</option>
                <option value="csv">CSV</option>
                <option value="qif">QIF</option>
                <option value="ofx">OFX</option>
              </select>
            </label>
            <label className="span-two"><span>默认分类（可选）</span>
              <select value={categoryId} onChange={(e) => setCategoryId(e.target.value)}>
                <option value="">不指定（仅 Koku 导出 CSV 可带分类）</option>
                {categories.map((category) => (
                  <option key={category.id} value={category.id}>
                    {category.kind === "income" ? "收入 · " : "支出 · "}{category.name}
                  </option>
                ))}
              </select>
            </label>
            <label><span>默认币种（可选）</span>
              <input value={currency} onChange={(e) => setCurrency(e.target.value)} placeholder="例如 CNY" />
            </label>
            <label className="span-two"><span>账单文件</span>
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
            <button type="button" className="secondary-button" onClick={onClose}>取消</button>
            <button className="primary-button" disabled={submitting || !file || !accountId}>
              {submitting ? <LoaderCircle className="spin" size={17} /> : <Upload size={16} />}
              {submitting ? "正在导入…" : "开始导入"}
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

/** 日期展示（如 "2026年8月15日"）。 */
function formatDay(value: string): string {
  return new Intl.DateTimeFormat("zh-CN", { year: "numeric", month: "long", day: "numeric" }).format(parseDay(value));
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
          setError(reason instanceof Error ? reason.message : "无法读取安全设置");
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const copy = async (text: string, which: "secret" | "uri") => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(which);
      window.setTimeout(() => setCopied(""), 1600);
    } catch {
      setError("复制失败，请手动选择复制");
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
      setError(reason instanceof Error ? reason.message : "设置失败");
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
      setNotice("已启用二步验证。请确认验证器已保存密钥，此后登录需输入动态码。");
      setStep("intro");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "启用失败");
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
      setNotice("二步验证已关闭。");
      setStep("intro");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "关闭失败");
    } finally {
      setBusy(false);
    }
  };

  return (
    <ModalShell eyebrow="TWO-FACTOR AUTH" title="二步验证" onClose={onClose}>
      <div className="entry-form">
        {loading ? (
          <div className="totp-loading"><LoaderCircle className="spin" size={18} /> 正在读取安全设置…</div>
        ) : step === "secret" ? (
          <>
            <p className="fx-hint">把下面的密钥添加到你的验证器（如 Google Authenticator、1Password），建议同时保存 otpauth 链接备用。</p>
            <div className="totp-secret-block">
              <span>账户密钥（Base32）</span>
              <div className="totp-secret-row">
                <code className="totp-secret">{secret}</code>
                <button type="button" className="copy-button" onClick={() => void copy(secret, "secret")}>
                  {copied === "secret" ? <Check size={13} /> : <Copy size={13} />}
                  {copied === "secret" ? "已复制" : "复制"}
                </button>
              </div>
            </div>
            <div className="totp-secret-block">
              <span>验证器链接（otpauth://）</span>
              <div className="totp-secret-row">
                <code className="totp-uri">{otpauthUri}</code>
                <button type="button" className="copy-button" onClick={() => void copy(otpauthUri, "uri")}>
                  {copied === "uri" ? <Check size={13} /> : <Copy size={13} />}
                  {copied === "uri" ? "已复制" : "复制"}
                </button>
              </div>
            </div>
            <form onSubmit={enable}>
              <div className="form-grid">
                <label className="span-two"><span>验证器动态码</span>
                  <input required autoFocus inputMode="numeric" maxLength={6} pattern="[0-9]*" value={code} onChange={(e) => setCode(e.target.value)} placeholder="输入 6 位动态码" />
                </label>
              </div>
              {error && <div className="form-error">{error}</div>}
              <div className="modal-actions">
                <button type="button" className="secondary-button" onClick={onClose}>取消</button>
                <button className="primary-button" disabled={busy || code.trim().length !== 6}>
                  {busy && <LoaderCircle className="spin" size={17} />}确认开启
                </button>
              </div>
            </form>
          </>
        ) : step === "password" ? (
          <form onSubmit={startSetup}>
            <div className="deposit-info"><p>开启前需要验证当前登录密码，防止他人擅自开启。</p></div>
            <div className="form-grid">
              <label className="span-two"><span>当前密码</span>
                <input required type="password" autoFocus autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} placeholder="输入当前密码" />
              </label>
            </div>
            {error && <div className="form-error">{error}</div>}
            <div className="modal-actions">
              <button type="button" className="secondary-button" onClick={() => { setError(null); setStep("intro"); }}>返回</button>
              <button className="primary-button" disabled={busy || !password}>{busy && <LoaderCircle className="spin" size={17} />}下一步</button>
            </div>
          </form>
        ) : enabled ? (
          <div className="totp-enabled">
            <p className="totp-status"><ShieldCheck size={17} /> 二步验证已启用</p>
            {notice && <div className="totp-notice" role="status"><Check size={14} /> {notice}</div>}
            {step === "disable" ? (
              <form onSubmit={disable}>
                <div className="deposit-info"><p>关闭后恢复仅凭密码登录。请输入验证器中的当前动态码确认。</p></div>
                <div className="form-grid">
                  <label className="span-two"><span>当前动态码</span>
                    <input required autoFocus inputMode="numeric" maxLength={6} pattern="[0-9]*" value={code} onChange={(e) => setCode(e.target.value)} placeholder="输入 6 位动态码" />
                  </label>
                </div>
                {error && <div className="form-error">{error}</div>}
                <div className="modal-actions">
                  <button type="button" className="secondary-button" onClick={() => { setError(null); setCode(""); setStep("intro"); }}>取消</button>
                  <button className="primary-button" disabled={busy || code.trim().length !== 6}>{busy && <LoaderCircle className="spin" size={17} />}关闭二步验证</button>
                </div>
              </form>
            ) : (
              <>
                <p className="fx-hint">每次登录都需要输入验证器中的 6 位动态码。如需关闭，请先验证当前动态码。</p>
                {error && <div className="form-error">{error}</div>}
                <div className="modal-actions">
                  <button type="button" className="secondary-button" onClick={onClose}>关闭</button>
                  <button type="button" className="primary-button" onClick={() => { setError(null); setNotice(null); setStep("disable"); }}><KeyRound size={16} />关闭二步验证</button>
                </div>
              </>
            )}
          </div>
        ) : (
          <>
            <p className="totp-intro-copy"><LockKeyhole size={17} /> 当前未开启二步验证</p>
            <p className="fx-hint">开启后，每次登录除密码外还需输入验证器生成的 6 位动态码，可防止密码泄露后被他人登录。</p>
            {notice && <div className="totp-notice" role="status"><Check size={14} /> {notice}</div>}
            {error && <div className="form-error">{error}</div>}
            <div className="modal-actions">
              <button type="button" className="secondary-button" onClick={onClose}>关闭</button>
              <button type="button" className="primary-button" onClick={() => { setError(null); setNotice(null); setStep("password"); }}><ShieldCheck size={16} />开始设置</button>
            </div>
          </>
        )}
      </div>
    </ModalShell>
  );
}

function ReconciliationStatusBadge({ status }: { status: ReconciliationStatus }) {
  const label = status === "open" ? "进行中" : status === "completed" ? "已完成" : "已取消";
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

  const refresh = async () => {
    try {
      setItems(await listReconciliations(account.id));
      setLoadError(null);
    } catch (reason) {
      setLoadError(reason instanceof Error ? reason.message : "加载对账记录失败");
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
      setError(reason instanceof Error ? reason.message : "新建对账失败");
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
      setError(reason instanceof Error ? reason.message : "完成对账失败");
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
      setError(reason instanceof Error ? reason.message : "取消对账失败");
    } finally {
      setBusyId(null);
    }
  };

  return (
    <ModalShell eyebrow="RECONCILE" title={`对账 · ${account.name}`} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="deposit-info">
          <p>当前账面余额 {formatMoney(account.balance, account.currency)}。完成对账时若与对账单有差额，将自动生成调整流水修正余额。</p>
        </div>
        <div className="form-grid">
          <label><span>对账日</span><input required type="date" value={date} onChange={(e) => setDate(e.target.value)} /></label>
          <label><span>对账单余额（{account.currency}）</span><input required step="0.01" inputMode="decimal" value={balance} onChange={(e) => setBalance(e.target.value)} placeholder="0.00" /></label>
          <label className="span-two"><span>备注（可选）</span><input value={note} onChange={(e) => setNote(e.target.value)} placeholder="例如：与银行流水核对无误" /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>关闭</button>
          <button className="primary-button" disabled={submitting || !date || !balance}>
            {submitting && <LoaderCircle className="spin" size={17} />}新建对账
          </button>
        </div>
      </form>

      <div className="reconcile-history">
        <div className="reconcile-history-head"><strong>历史对账</strong><small>{items ? `${items.length} 笔` : ""}</small></div>
        {loadError && <div className="form-error">{loadError}</div>}
        {items === null ? (
          loadError ? null : <div className="empty-hint"><LoaderCircle className="spin" size={16} /> 正在加载…</div>
        ) : items.length === 0 ? (
          <div className="empty-hint">还没有对账记录。</div>
        ) : (
          <div className="reconcile-list">
            {items.map((item) => (
              <div className={`reconcile-item ${item.status}`} key={item.id}>
                <div className="reconcile-item-head">
                  <strong>{formatDay(item.statement_date)}</strong>
                  <ReconciliationStatusBadge status={item.status} />
                </div>
                <div className="reconcile-item-meta">
                  <span>对账单 {formatMoney(item.statement_balance, account.currency)}</span>
                  <span>账面 {formatMoney(item.book_balance, account.currency)}</span>
                  <span>开始于 {formatDate(item.opened_at)}</span>
                </div>
                {item.note && <p className="fx-hint">{item.note}</p>}
                {item.completed_at && <p className="fx-hint">完成于 {formatDate(item.completed_at)}</p>}
                {item.adjustment_transaction_id != null && (
                  <p className="reconcile-adjustment"><RotateCcw size={12} /> 已自动生成调整流水，余额已修正</p>
                )}
                {item.status === "open" && (
                  <div className="reconcile-actions">
                    <button type="button" className="text-button" disabled={busyId === item.id} onClick={() => void complete(item)}>
                      {busyId === item.id ? <LoaderCircle className="spin" size={13} /> : <ClipboardCheck size={13} />}完成对账
                    </button>
                    <button type="button" className="text-button danger" disabled={busyId === item.id} onClick={() => void cancel(item)}>
                      <X size={13} />取消
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
