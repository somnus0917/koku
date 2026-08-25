//! 总览页：净值趋势、月度现金流、账户条与近期流水。
import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDownLeft, ArrowUpRight, BellRing, CircleDollarSign, Eye, EyeOff, MoreHorizontal, Plus, Target } from "lucide-react";
import { createSavingsGoal, deleteSavingsGoal, getSavingsGoals, updateSavingsGoal, type SavingsGoalInput } from "../../api";
import { ModalShell } from "../../components/ModalShell";
import { PageTitle } from "../../components/PageTitle";
import { accountIcon, convertedMoney, useConversionRates } from "../../components/accountDisplay";
import { TransactionList } from "../transactions/TransactionList";
import { CategoryBars } from "../insights/CategoryBars";
import { formatDate, formatMoney, healthScore } from "../../lib";
import { uiLocale } from "../../i18n";
import type { Account, AppData, Deposit, Loan, NetWorthSnapshot, SavingsGoal } from "../../types";

function ReminderBanner({ deposits, loans }: { deposits: Deposit[]; loans: Loan[] }) {
  const { t } = useTranslation();
  const now = Date.now();
  const maturedDeposits = deposits.filter(
    (deposit) => !deposit.settled_at && new Date(deposit.maturity_at).getTime() <= now
  );
  const overdueLoans = loans.filter(
    (loan) => !loan.closed_at && loan.due_at && new Date(loan.due_at).getTime() <= now
  );
  if (maturedDeposits.length === 0 && overdueLoans.length === 0) return null;
  return (
    <aside className="reminder-banner" role="status">
      <BellRing size={18} />
      <div>
        {maturedDeposits.map((deposit) => (
          <p key={deposit.id}>{t("dashboard.reminderDepositMatured", { days: deposit.term_days, date: formatDate(deposit.maturity_at) })}</p>
        ))}
        {overdueLoans.map((loan) => (
          <p key={loan.id}>
            {t("dashboard.reminderLoanDue", {
              type: t(loan.loan_type === "lend" ? "common.lend" : "common.borrow"),
              counterparty: loan.counterparty,
              date: formatDate(loan.due_at!)
            })}
          </p>
        ))}
      </div>
    </aside>
  );
}
export function Dashboard({
  data,
  onAdd,
  onShowTransactions
}: {
  data: AppData;
  onAdd: () => void;
  onShowTransactions: () => void;
}) {
  const [hidden, setHidden] = useState(false);
  const { t } = useTranslation();
  const activeTransactions = data.transactions.filter((item) => !item.voided_at);
  const recent = activeTransactions.slice(0, 5);
  const display = data.monthly.currency;
  const rateCurrencies = useMemo(
    () => [
      ...new Set([
        ...data.accounts.map((account) => account.currency),
        ...data.transactions.map((item) => item.currency)
      ])
    ],
    [data.accounts, data.transactions]
  );
  const rates = useConversionRates(rateCurrencies, display);
  return (
    <div className="page page-enter">
      <PageTitle eyebrow="WELCOME BACK" title={t("dashboard.greeting")} />
      <ReminderBanner deposits={data.deposits} loans={data.loans} />
      <section className="hero-grid">
        <article className="net-worth-card">
          <div className="card-heading">
            <span>{t("dashboard.netWorth", { currency: data.balance.currency })}</span>
            <button className="bare-button" onClick={() => setHidden((value) => !value)} aria-label={t("dashboard.hideAmounts")}>
              {hidden ? <EyeOff size={18} /> : <Eye size={18} />}
            </button>
          </div>
          <strong className="hero-amount">
            {hidden ? "••••••" : formatMoney(data.balance.net_worth, data.balance.currency)}
          </strong>
          <div className="hero-meta">
            <span className={Number(data.monthly.net) >= 0 ? "positive" : "negative"}>
              {Number(data.monthly.net) >= 0 ? <ArrowUpRight size={15} /> : <ArrowDownLeft size={15} />}
              {t("dashboard.monthlyNet")} {hidden ? "••••" : formatMoney(data.monthly.net, data.monthly.currency)}
            </span>
            <span>{t("dashboard.accountsConnected", { count: data.accounts.length })}</span>
          </div>
          <NetWorthTrendChart snapshots={data.netWorthTrend} />
        </article>

        <article className="month-card">
          <div className="card-heading">
            <span>{t("dashboard.monthCashFlow", { month: data.monthly.month })}</span>
            <CircleDollarSign size={19} />
          </div>
          <div className="flow-row income">
            <span className="flow-icon"><ArrowDownLeft size={18} /></span>
            <div><small>{t("common.income")}</small><strong>{hidden ? "••••" : formatMoney(data.monthly.total_income, data.monthly.currency)}</strong></div>
          </div>
          <div className="flow-row expense">
            <span className="flow-icon"><ArrowUpRight size={18} /></span>
            <div><small>{t("common.expense")}</small><strong>{hidden ? "••••" : formatMoney(data.monthly.total_expense, data.monthly.currency)}</strong></div>
          </div>
          <div className="saving-rate">
            <span>{t("dashboard.healthScore")}</span>
            <strong>{healthScore(data.monthly)}%</strong>
            <div><i style={{ width: `${healthScore(data.monthly)}%` }} /></div>
          </div>
        </article>
      </section>

      <section className="section-block">
        <div className="section-heading">
          <div><span>ACCOUNTS</span><h2>{t("dashboard.yourAccounts")}</h2></div>
          <button className="text-button" onClick={onAdd}><Plus size={16} /> {t("dashboard.quickAdd")}</button>
        </div>
        <div className="account-strip">
          {data.accounts.map((account, index) => (
            <AccountMiniCard key={account.id} account={account} hidden={hidden} index={index} display={display} rates={rates} />
          ))}
        </div>
      </section>

      <SavingsGoals accounts={data.accounts} currency={display} />

      <section className="dashboard-lower">
        <article className="panel recent-panel">
          <div className="section-heading compact-heading">
            <div><span>ACTIVITY</span><h2>{t("dashboard.recentTransactions")}</h2></div>
            <button className="text-button" onClick={onShowTransactions}>{t("dashboard.viewAll")}</button>
          </div>
          <TransactionList
            transactions={recent}
            accounts={data.accounts}
            categories={data.categories}
            display={display}
            rates={rates}
          />
        </article>
        <article className="panel categories-panel">
          <div className="section-heading compact-heading">
            <div><span>SPENDING</span><h2>{t("dashboard.spending")}</h2></div>
          </div>
          <CategoryBars summary={data.monthly} />
        </article>
      </section>
    </div>
  );
}

function SavingsGoals({ accounts, currency }: { accounts: Account[]; currency: string }) {
  const [goals, setGoals] = useState<SavingsGoal[]>([]);
  const [editing, setEditing] = useState<SavingsGoal | null>(null);
  const [creating, setCreating] = useState(false);
  const load = useCallback(async () => { try { setGoals(await getSavingsGoals()); } catch { /* 总览其他信息仍可显示 */ } }, []);
  useEffect(() => { void load(); }, [load]);
  const accountMap = useMemo(() => new Map(accounts.map((item) => [item.id, item])), [accounts]);
  const remove = async (id: number) => { if (!window.confirm("确认删除这个储蓄目标？")) return; await deleteSavingsGoal(id); await load(); };
  return <section className="section-block goals-overview"><div className="section-heading"><div><span>GOALS</span><h2>储蓄目标</h2></div><button className="text-button" onClick={() => setCreating(true)}><Plus size={16} /> 新建目标</button></div><div className="goal-cards">{goals.map((item) => { const progress = Math.min(100, Number(item.current_amount) / Number(item.target_amount) * 100 || 0); const goalCurrency = accountMap.get(item.account_id ?? 0)?.currency ?? currency; return <article className="goal-card" key={item.id}><div className="goal-card-heading"><span><Target size={17} /></span><button className="bare-button" onClick={() => setEditing(item)} aria-label="编辑目标"><MoreHorizontal size={18} /></button></div><h3>{item.name}</h3><strong>{formatMoney(item.current_amount, goalCurrency)} <small>/ {formatMoney(item.target_amount, goalCurrency)}</small></strong><div className="goal-track"><i style={{ width: `${progress}%` }} /></div><footer><span>{progress.toFixed(0)}%</span><span>{item.target_date ?? "未设截止日期"}</span></footer><button className="text-button goal-delete" onClick={() => void remove(item.id)}>删除</button></article>; })}{goals.length === 0 && <article className="goal-empty"><Target size={20} /><div><strong>给未来留一点空间</strong><span>设定旅行、应急金或其他储蓄目标，在这里查看进度。</span></div></article>}</div>{(creating || editing) && <GoalModal goal={editing} accounts={accounts} onClose={() => { setCreating(false); setEditing(null); }} onSaved={() => void load()} />}</section>;
}
function GoalModal({ goal, accounts, onClose, onSaved }: { goal: SavingsGoal | null; accounts: Account[]; onClose: () => void; onSaved: () => void }) { const [draft, setDraft] = useState<SavingsGoalInput>(() => goal ? { name: goal.name, account_id: goal.account_id, target_amount: goal.target_amount, current_amount: goal.current_amount, target_date: goal.target_date } : { name: "", account_id: null, target_amount: "", current_amount: "0", target_date: null }); const [busy, setBusy] = useState(false); const save = async (event: FormEvent) => { event.preventDefault(); setBusy(true); try { if (goal) await updateSavingsGoal(goal.id, draft); else await createSavingsGoal(draft); onSaved(); onClose(); } finally { setBusy(false); } }; return <ModalShell eyebrow="SAVINGS GOAL" title={goal ? "编辑储蓄目标" : "新建储蓄目标"} onClose={onClose}><form className="entry-form" onSubmit={(event) => void save(event)}><div className="form-grid"><label><span>目标名称</span><input required value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} placeholder="例如：旅行基金" /></label><label><span>目标金额</span><input required type="number" min="0.01" step="0.01" value={draft.target_amount} onChange={(e) => setDraft({ ...draft, target_amount: e.target.value })} /></label><label><span>当前已存</span><input required type="number" min="0" step="0.01" value={draft.current_amount} onChange={(e) => setDraft({ ...draft, current_amount: e.target.value })} /></label><label><span>关联账户</span><select value={draft.account_id ?? ""} onChange={(e) => setDraft({ ...draft, account_id: e.target.value ? Number(e.target.value) : null })}><option value="">不关联</option>{accounts.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label><label className="span-two"><span>目标日期</span><input type="date" value={draft.target_date ?? ""} onChange={(e) => setDraft({ ...draft, target_date: e.target.value || null })} /></label></div><div className="modal-actions"><button type="button" className="secondary-button" onClick={onClose}>取消</button><button className="primary-button" disabled={busy}>{busy ? "保存中" : "保存"}</button></div></form></ModalShell>; }
function NetWorthTrendChart({ snapshots }: { snapshots: NetWorthSnapshot[] }) {
  const values = snapshots.map((snapshot) => Number(snapshot.net_worth));
  const points = values.length === 0 ? [0, 0] : values.length === 1 ? [values[0], values[0]] : values;
  const min = Math.min(...points, 0);
  const max = Math.max(...points, 1);
  const range = Math.max(1, max - min);
  const coords = points.map((value, index) => ({
    x: 10 + (index / (points.length - 1)) * 700,
    y: 190 - ((value - min) / range) * 155
  }));
  const line = coords.map((point, index) => `${index ? "L" : "M"}${point.x.toFixed(1)},${point.y.toFixed(1)}`).join(" ");
  const area = `${line} L710,205 L10,205 Z`;
  const { t } = useTranslation();
  const formatDay = (value: string) => new Intl.DateTimeFormat(uiLocale(), { month: "short", day: "numeric" }).format(new Date(`${value}T00:00:00`));
  const labels = snapshots.length === 0
    ? [t("dashboard.today"), t("dashboard.today"), t("dashboard.today")]
    : [snapshots[0], snapshots[Math.floor((snapshots.length - 1) / 2)], snapshots[snapshots.length - 1]].map((snapshot) => formatDay(snapshot.snapshot_date));
  return (
    <div className="trend-chart" aria-label={t("dashboard.netWorthTrendAria")}>
      <span className="trend-snapshot-note">{t("dashboard.snapshotTrend")}</span>
      <svg viewBox="0 0 720 220" role="img">
        <defs>
          <linearGradient id="trend-fill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="var(--accent)" stopOpacity="0.2" />
            <stop offset="100%" stopColor="var(--accent)" stopOpacity="0" />
          </linearGradient>
        </defs>
        {[45, 95, 145, 195].map((y) => <line key={y} x1="10" x2="710" y1={y} y2={y} className="grid-line" />)}
        <path d={area} fill="url(#trend-fill)" />
        <path d={line} className="trend-line" />
        <circle cx={coords.at(-1)?.x} cy={coords.at(-1)?.y} r="4.5" className="trend-dot" />
      </svg>
      <div className="chart-labels"><span>{labels[0]}</span><span>{labels[1]}</span><span>{labels[2]}</span></div>
    </div>
  );
}
export function AccountMiniCard({
  account,
  hidden,
  index,
  display,
  rates
}: {
  account: Account;
  hidden: boolean;
  index: number;
  display: string;
  rates: Record<string, number>;
}) {
  const Icon = accountIcon(account);
  const { t } = useTranslation();
  const shown = convertedMoney(account.balance, account.currency, display, rates)
    ?? { amount: account.balance, currency: account.currency };
  const isConverted = shown.currency !== account.currency;
  return (
    <article
      className={`account-mini tone-${index % 4}`}
      title={isConverted ? t("accounts.originalCurrency", { amount: formatMoney(account.balance, account.currency) }) : undefined}
    >
      <div><span className="account-icon"><Icon size={19} /></span><MoreHorizontal size={18} /></div>
      <small>{account.account_type === "credit" ? t("accounts.type.credit") : t(`accounts.typeCard.${account.account_type}`)}</small>
      <h3>{account.name}</h3>
      <strong>{hidden ? "••••••" : formatMoney(shown.amount, shown.currency)}</strong>
      <span className="currency-badge">{shown.currency}</span>
    </article>
  );
}
