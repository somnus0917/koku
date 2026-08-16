//! 弹窗组件：新建/编辑账户、交易、分类、定期、报销、借款。
import { useEffect, useState, type CSSProperties, type FormEvent } from "react";
import {
  BadgeDollarSign,
  Check,
  ChevronDown,
  CircleDollarSign,
  LoaderCircle,
  PiggyBank,
  RefreshCcw,
  RotateCcw,
  X,
  type LucideIcon
} from "lucide-react";
import { rateHint } from "../api";
import type { createTransaction, createTransfer } from "../api";
import {
  availableCurrencies,
  formatDate,
  formatMoney,
  localDateTimeValue,
  toLocalDateTimeValue
} from "../lib";
import { CategoryAvatar } from "./avatar";
import type {
  Account,
  AccountType,
  Category,
  CategoryKind,
  Loan,
  LoanType,
  RateQuote,
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
  deposit: Account;
  accounts: Account[];
  onClose: () => void;
  onSubmit: (toAccountId: number) => Promise<void>;
}) {
  const [targetId, setTargetId] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const targets = accounts.filter((account) => account.id !== deposit.id);
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
          <p><strong>{deposit.name}</strong> · 年利率 {deposit.interest_rate}%{deposit.maturity_at ? ` · ${formatDate(deposit.maturity_at)} 到期` : ""}</p>
          <p>当前本金 {formatMoney(deposit.balance, deposit.currency)}，结清时按实际持有天数计息，本息一并转回。</p>
        </div>
        <div className="form-grid">
          <label className="span-two"><span>转回账户</span>
            <select required value={targetId} onChange={(e) => setTargetId(e.target.value)}>
              <option value="" disabled>选择目标账户</option>
              {targets.map((account) => (
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
  onSubmit: (input: { loan_type: LoanType; counterparty: string; amount: string; account_id: number; note?: string }) => Promise<void>;
}) {
  const [loanType, setLoanType] = useState<LoanType>("lend");
  const [counterparty, setCounterparty] = useState("");
  const [accountId, setAccountId] = useState("");
  const [amount, setAmount] = useState("");
  const [note, setNote] = useState("");
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
        note: note || undefined
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
  onClose,
  onSubmit
}: {
  accounts: Account[];
  categories: Category[];
  onClose: () => void;
  onSubmit: (input: TransactionSubmit) => Promise<void>;
}) {
  const [kind, setKind] = useState<Exclude<TransactionKind, "loan" | "adjustment">>("expense");
  const [accountId, setAccountId] = useState(accounts[0]?.id ?? 0);
  const [targetId, setTargetId] = useState(accounts[1]?.id ?? accounts[0]?.id ?? 0);
  const [sourceCurrency, setSourceCurrency] = useState(accounts[0]?.currency ?? "CNY");
  const [categoryId, setCategoryId] = useState(categories.find((item) => item.kind === "expense")?.id ?? 0);
  const [amount, setAmount] = useState("");
  const [settledAmount, setSettledAmount] = useState("");
  const [settledTouched, setSettledTouched] = useState(false);
  const [targetAmount, setTargetAmount] = useState("");
  const [note, setNote] = useState("");
  const [occurredAt, setOccurredAt] = useState(localDateTimeValue);
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

  const changeKind = (nextKind: Exclude<TransactionKind, "loan" | "adjustment">) => {
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
            note
          }
        });
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
  onClose,
  onSubmit
}: {
  transaction: Transaction;
  accounts: Account[];
  categories: Category[];
  onClose: () => void;
  onSubmit: (input: {
    note?: string;
    occurred_at?: string;
    category_id?: number;
    amount?: string;
    account_id?: number;
    settled_amount?: string;
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
