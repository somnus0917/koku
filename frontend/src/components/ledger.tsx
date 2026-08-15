//! 页面级组件：总览、账户、交易、分析、借贷区块。
import { useMemo, useState, type CSSProperties, type ReactNode } from "react";
import {
  ArrowDownLeft,
  ArrowLeftRight,
  ArrowUpRight,
  BadgeDollarSign,
  Banknote,
  ChartNoAxesCombined,
  ChevronDown,
  CircleDollarSign,
  CreditCard,
  Eye,
  EyeOff,
  Handshake,
  LayoutDashboard,
  MoreHorizontal,
  PiggyBank,
  Plus,
  ReceiptText,
  RefreshCcw,
  RotateCcw,
  Search,
  ShieldCheck,
  Tags,
  Trash2,
  TrendingUp,
  WalletCards,
  X,
  type LucideIcon
} from "lucide-react";
import { buildDonutGradient, categoryVisual, formatDate, formatMoney, healthScore } from "../lib";
import { CategoryAvatar } from "./avatar";
import type {
  Account,
  AccountType,
  AppData,
  CashFlowSummary,
  Category,
  CategoryKind,
  Loan,
  MonthlySummary,
  Transaction,
  TransactionKind
} from "../types";

export function accountIcon(account: Account): LucideIcon {
  if (account.account_type === "savings") return PiggyBank;
  if (account.account_type === "stock") return TrendingUp;
  if (account.account_type === "credit") return CreditCard;
  if (account.name.includes("现金")) return Banknote;
  return WalletCards;
}

export function PageTitle({ eyebrow, title, actions }: { eyebrow: string; title: string; actions?: React.ReactNode }) {
  return (
    <div className="page-title">
      <div>
        <span>{eyebrow}</span>
        <h1>{title}</h1>
      </div>
      {actions && <div className="page-actions">{actions}</div>}
    </div>
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
  const activeTransactions = data.transactions.filter((item) => !item.voided_at);
  const recent = activeTransactions.slice(0, 5);
  return (
    <div className="page page-enter">
      <PageTitle eyebrow="WELCOME BACK" title="今天，也把生活记清楚。" />
      <section className="hero-grid">
        <article className="net-worth-card">
          <div className="card-heading">
            <span>净资产 · {data.balance.currency}</span>
            <button className="bare-button" onClick={() => setHidden((value) => !value)} aria-label="隐藏金额">
              {hidden ? <EyeOff size={18} /> : <Eye size={18} />}
            </button>
          </div>
          <strong className="hero-amount">
            {hidden ? "••••••" : formatMoney(data.balance.net_worth, data.balance.currency)}
          </strong>
          <div className="hero-meta">
            <span className={Number(data.monthly.net) >= 0 ? "positive" : "negative"}>
              {Number(data.monthly.net) >= 0 ? <ArrowUpRight size={15} /> : <ArrowDownLeft size={15} />}
              本月结余 {hidden ? "••••" : formatMoney(data.monthly.net, data.monthly.currency)}
            </span>
            <span>{data.accounts.length} 个账户已连接</span>
          </div>
          <TrendChart transactions={activeTransactions} currency={data.monthly.currency} />
        </article>

        <article className="month-card">
          <div className="card-heading">
            <span>{data.monthly.month} 月现金流</span>
            <CircleDollarSign size={19} />
          </div>
          <div className="flow-row income">
            <span className="flow-icon"><ArrowDownLeft size={18} /></span>
            <div><small>收入</small><strong>{hidden ? "••••" : formatMoney(data.monthly.total_income, data.monthly.currency)}</strong></div>
          </div>
          <div className="flow-row expense">
            <span className="flow-icon"><ArrowUpRight size={18} /></span>
            <div><small>支出</small><strong>{hidden ? "••••" : formatMoney(data.monthly.total_expense, data.monthly.currency)}</strong></div>
          </div>
          <div className="saving-rate">
            <span>收支健康度</span>
            <strong>{healthScore(data.monthly)}%</strong>
            <div><i style={{ width: `${healthScore(data.monthly)}%` }} /></div>
          </div>
        </article>
      </section>

      <section className="section-block">
        <div className="section-heading">
          <div><span>ACCOUNTS</span><h2>你的账户</h2></div>
          <button className="text-button" onClick={onAdd}><Plus size={16} /> 快速记账</button>
        </div>
        <div className="account-strip">
          {data.accounts.map((account, index) => (
            <AccountMiniCard key={account.id} account={account} hidden={hidden} index={index} />
          ))}
        </div>
      </section>

      <section className="dashboard-lower">
        <article className="panel recent-panel">
          <div className="section-heading compact-heading">
            <div><span>ACTIVITY</span><h2>最近交易</h2></div>
            <button className="text-button" onClick={onShowTransactions}>查看全部</button>
          </div>
          <TransactionList transactions={recent} accounts={data.accounts} categories={data.categories} />
        </article>
        <article className="panel categories-panel">
          <div className="section-heading compact-heading">
            <div><span>SPENDING</span><h2>支出去向</h2></div>
          </div>
          <CategoryBars summary={data.monthly} />
        </article>
      </section>
    </div>
  );
}

export function TrendChart({ transactions, currency }: { transactions: Transaction[]; currency: string }) {
  const points = useMemo(() => {
    const values = Array.from({ length: 12 }, (_, index) => ({ x: index, value: 0 }));
    for (const item of transactions) {
      if (item.kind === "transfer" || item.kind === "loan" || item.kind === "adjustment" || item.currency !== currency) continue;
      const day = new Date(item.occurred_at).getDate();
      const bucket = Math.min(11, Math.floor(((day - 1) / 31) * 12));
      const signed = item.kind === "income" ? Number(item.amount) : -Number(item.amount);
      values[bucket].value += signed;
    }
    let running = 0;
    return values.map((item) => {
      running += item.value;
      return running;
    });
  }, [currency, transactions]);
  const min = Math.min(...points, 0);
  const max = Math.max(...points, 1);
  const range = Math.max(1, max - min);
  const coords = points.map((value, index) => ({
    x: 10 + (index / (points.length - 1)) * 700,
    y: 190 - ((value - min) / range) * 155
  }));
  const line = coords.map((point, index) => `${index ? "L" : "M"}${point.x.toFixed(1)},${point.y.toFixed(1)}`).join(" ");
  const area = `${line} L710,205 L10,205 Z`;
  return (
    <div className="trend-chart" aria-label="本月现金流趋势图">
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
      <div className="chart-labels"><span>月初</span><span>月中</span><span>今天</span></div>
    </div>
  );
}

export function AccountMiniCard({ account, hidden, index }: { account: Account; hidden: boolean; index: number }) {
  const Icon = accountIcon(account);
  return (
    <article className={`account-mini tone-${index % 4}`}>
      <div><span className="account-icon"><Icon size={19} /></span><MoreHorizontal size={18} /></div>
      <small>{account.account_type === "credit" ? "信用" : ({ cash: "零钱账户", savings: "储蓄账户", stock: "股票账户" } as Record<AccountType, string>)[account.account_type]}</small>
      <h3>{account.name}</h3>
      <strong>{hidden ? "••••••" : formatMoney(account.balance, account.currency)}</strong>
      <span className="currency-badge">{account.currency}</span>
    </article>
  );
}

export function AccountsPage({
  data,
  onAddAccount,
  onEdit,
  onDeposit,
  onSettle,
  onCreateLoan,
  onRepay
}: {
  data: AppData;
  onAddAccount: () => void;
  onEdit: (account: Account) => void;
  onDeposit: (account: Account) => void;
  onSettle: (account: Account) => void;
  onCreateLoan: () => void;
  onRepay: (loan: Loan) => void;
}) {
  const group = (type: AccountType) => data.accounts.filter((account) => account.account_type === type);
  const cash = group("cash");
  const savings = group("savings");
  const stock = group("stock");
  const credit = group("credit");
  return (
    <div className="page page-enter">
      <PageTitle
        eyebrow="ACCOUNTS"
        title="账户"
        actions={<button className="primary-button" onClick={onAddAccount}><Plus size={18} /> 新建账户</button>}
      />
      <section className="balance-summary-row">
        <SummaryCard label="总资产" value={data.balance.total_assets} currency={data.balance.currency} tone="green" />
        <SummaryCard label="总负债" value={data.balance.total_liabilities} currency={data.balance.currency} tone="orange" />
        <SummaryCard label="净资产" value={data.balance.net_worth} currency={data.balance.currency} tone="blue" />
      </section>
      <AccountGroup title="零钱" subtitle={`${cash.length} 个账户`} accounts={cash} onEdit={onEdit} />
      <AccountGroup
        title="储蓄"
        subtitle={`${savings.length} 个账户`}
        accounts={savings}
        onEdit={onEdit}
        headingAction={savings.length > 0 ? <button className="text-button" onClick={() => onDeposit(savings[0])}><PiggyBank size={16} /> 转定期</button> : undefined}
        renderAction={(account) => (
          account.interest_rate
            ? <button className="row-action" title="结清定期并转回" aria-label="结清定期" onClick={() => onSettle(account)}><RotateCcw size={16} /></button>
            : <button className="row-action" title="转入定期" aria-label="转入定期" onClick={() => onDeposit(account)}><PiggyBank size={16} /></button>
        )}
      />
      <AccountGroup title="股票" subtitle={`${stock.length} 个账户`} accounts={stock} onEdit={onEdit} />
      <AccountGroup title="信用" subtitle={`${credit.length} 个账户`} accounts={credit} onEdit={onEdit} />
      <LoansSection
        loans={data.loans}
        accounts={data.accounts}
        onCreateLoan={onCreateLoan}
        onRepay={onRepay}
      />
    </div>
  );
}

export function SummaryCard({ label, value, currency, tone }: { label: string; value: string; currency: string; tone: string }) {
  return (
    <article className={`summary-card ${tone}`}>
      <span>{label}</span>
      <strong>{formatMoney(value, currency)}</strong>
      <small>以 {currency} 计价</small>
    </article>
  );
}

export function AccountGroup({
  title,
  subtitle,
  accounts,
  onEdit,
  renderAction,
  headingAction
}: {
  title: string;
  subtitle: string;
  accounts: Account[];
  onEdit?: (account: Account) => void;
  renderAction?: (account: Account) => React.ReactNode;
  headingAction?: React.ReactNode;
}) {
  return (
    <section className="section-block account-group">
      <div className="section-heading compact-heading">
        <div><span>{subtitle}</span><h2>{title}</h2></div>
        {headingAction}
      </div>
      <div className="account-grid">
        {accounts.map((account, index) => {
          const Icon = accountIcon(account);
          return (
            <article className="account-detail-card" key={account.id}>
              <span className={`large-account-icon tone-${index % 4}`}><Icon size={23} /></span>
              <div className="account-detail-copy">
                <h3>{account.name}</h3>
                <span>
                  {account.currency} 结算 · 单一余额
                  {account.credit_limit
                    ? ` · 额度 ${formatMoney(account.credit_limit, account.currency)} · 已用 ${formatMoney(account.balance, account.currency)}`
                    : account.interest_rate && account.maturity_at
                      ? ` · 定期 ${account.interest_rate}% · ${formatDate(account.maturity_at)}到期`
                      : ""}
                </span>
              </div>
              <strong>{formatMoney(account.balance, account.currency)}</strong>
              {renderAction ? renderAction(account) : null}
              <button className="bare-button" aria-label={`编辑${account.name}`} title="编辑账户" onClick={() => onEdit?.(account)}><MoreHorizontal size={19} /></button>
            </article>
          );
        })}
        {accounts.length === 0 && <EmptyState title="这里还没有账户" detail="新建账户后即可开始记账。" />}
      </div>
    </section>
  );
}

export function TransactionsPage({
  data,
  onAdd,
  onVoid,
  onMarkReimbursable,
  onUnmarkReimbursable,
  onReimburse
}: {
  data: AppData;
  onAdd: () => void;
  onVoid: (transaction: Transaction) => void;
  onMarkReimbursable: (transaction: Transaction) => void;
  onUnmarkReimbursable: (transaction: Transaction) => void;
  onReimburse: (transaction: Transaction) => void;
}) {
  const [search, setSearch] = useState("");
  const [kind, setKind] = useState<"all" | TransactionKind>("all");
  const accountsById = useMemo(() => new Map(data.accounts.map((item) => [item.id, item])), [data.accounts]);
  const categoriesById = useMemo(() => new Map(data.categories.map((item) => [item.id, item])), [data.categories]);
  const filtered = data.transactions.filter((item) => {
    const category = item.category_id ? categoriesById.get(item.category_id)?.name ?? "" : "转账";
    const account = accountsById.get(item.account_id)?.name ?? "";
    const matchesSearch = `${item.note} ${category} ${account}`.toLowerCase().includes(search.toLowerCase());
    return matchesSearch && (kind === "all" || item.kind === kind);
  });
  return (
    <div className="page page-enter">
      <PageTitle
        eyebrow="TRANSACTIONS"
        title="交易流水"
        actions={<button className="primary-button" onClick={onAdd}><Plus size={18} /> 记一笔</button>}
      />
      <div className="transaction-toolbar">
        <label className="search-box"><Search size={18} /><input value={search} onChange={(e) => setSearch(e.target.value)} placeholder="搜索备注、分类或账户" /></label>
        <div className="segmented-filter">
          {(["all", "expense", "income", "transfer", "loan"] as const).map((item) => (
            <button key={item} className={kind === item ? "active" : ""} onClick={() => setKind(item)}>
              {{ all: "全部", expense: "支出", income: "收入", transfer: "转账", loan: "借贷" }[item]}
            </button>
          ))}
        </div>
      </div>
      <article className="panel transaction-table">
        <div className="table-header"><span>交易</span><span>账户</span><span>日期</span><span>金额</span><span /></div>
        {filtered.map((transaction) => (
          <TransactionRow
            key={transaction.id}
            transaction={transaction}
            account={accountsById.get(transaction.account_id)}
            target={transaction.to_account_id ? accountsById.get(transaction.to_account_id) : undefined}
            category={transaction.category_id ? categoriesById.get(transaction.category_id) : undefined}
            onVoid={() => onVoid(transaction)}
            onMarkReimbursable={() => onMarkReimbursable(transaction)}
            onUnmarkReimbursable={() => onUnmarkReimbursable(transaction)}
            onReimburse={() => onReimburse(transaction)}
          />
        ))}
        {filtered.length === 0 && <EmptyState title="没有找到交易" detail="换个关键词，或记录一笔新的交易。" />}
      </article>
    </div>
  );
}

export function TransactionList({ transactions, accounts, categories }: { transactions: Transaction[]; accounts: Account[]; categories: Category[] }) {
  const accountMap = new Map(accounts.map((item) => [item.id, item]));
  const categoryMap = new Map(categories.map((item) => [item.id, item]));
  return (
    <div className="simple-list">
      {transactions.map((transaction) => (
        <TransactionRow
          compact
          key={transaction.id}
          transaction={transaction}
          account={accountMap.get(transaction.account_id)}
          target={transaction.to_account_id ? accountMap.get(transaction.to_account_id) : undefined}
          category={transaction.category_id ? categoryMap.get(transaction.category_id) : undefined}
        />
      ))}
      {transactions.length === 0 && <EmptyState title="还没有交易" detail="点击“记一笔”开始。" />}
    </div>
  );
}

export function TransactionRow({
  transaction,
  account,
  target,
  category,
  compact = false,
  onVoid,
  onMarkReimbursable,
  onUnmarkReimbursable,
  onReimburse
}: {
  transaction: Transaction;
  account?: Account;
  target?: Account;
  category?: Category;
  compact?: boolean;
  onVoid?: () => void;
  onMarkReimbursable?: () => void;
  onUnmarkReimbursable?: () => void;
  onReimburse?: () => void;
}) {
  const meta = {
    expense: { icon: ArrowUpRight, label: category?.name ?? "支出", className: "expense" },
    income: { icon: ArrowDownLeft, label: category?.name ?? "收入", className: "income" },
    transfer: { icon: ArrowLeftRight, label: "账户转账", className: "transfer" },
    loan: { icon: Handshake, label: transaction.note || "借款", className: "transfer" },
    adjustment: { icon: RotateCcw, label: "余额调整", className: "transfer" }
  }[transaction.kind];
  const Icon = meta.icon;
  const prefix =
    transaction.kind === "expense" ? "−"
    : transaction.kind === "income" ? "+"
    : transaction.kind === "adjustment" ? (Number(transaction.amount) > 0 ? "+" : "")
    : "";
  const reimbursable = transaction.reimbursable_at && !transaction.reimbursed_at;
  // 只有可操作的支出行才有报销列（避免其他行多出一段空列）。
  const hasReimburseActions = transaction.kind === "expense" && !transaction.voided_at && !transaction.reimbursed_at;
  return (
    <div className={`transaction-row ${compact ? "compact-row" : ""} ${transaction.voided_at ? "voided" : ""} ${hasReimburseActions ? "has-reimburse" : ""}`}>
      <div className="transaction-main">
        {transaction.kind === "transfer" || transaction.kind === "loan" || transaction.kind === "adjustment" ? (
          <span className={`transaction-icon ${meta.className}`}><Icon size={18} /></span>
        ) : (
          <CategoryAvatar name={meta.label} className={`transaction-icon ${meta.className}`} />
        )}
        <div>
          <strong>
            {transaction.note || meta.label}
            {transaction.voided_at ? " · 已撤销" : ""}
            {reimbursable ? <span className="reimburse-badge">待报销</span> : ""}
            {transaction.reimbursed_at ? <span className="reimburse-badge done">已报销</span> : ""}
          </strong>
          <span>{meta.label}</span>
        </div>
      </div>
      {!compact && <span className="table-account">{account?.name ?? "未知账户"}{target ? ` → ${target.name}` : ""}</span>}
      {!compact && <span className="table-date">{formatDate(transaction.occurred_at)}</span>}
      <div className={`transaction-amount ${meta.className}`}>
        <strong>{prefix}{formatMoney(transaction.amount, transaction.currency)}</strong>
        {transaction.kind === "transfer" && transaction.target_amount && transaction.target_currency && (
          <span>到账 {formatMoney(transaction.target_amount, transaction.target_currency)}</span>
        )}
        {transaction.kind !== "transfer" && transaction.kind !== "loan" && transaction.kind !== "adjustment" && account && transaction.currency !== account.currency && (
          <span>入账 {formatMoney(transaction.settled_amount, account.currency)}</span>
        )}
        {transaction.kind === "expense" && transaction.reimbursed_amount !== "0" && (
          <span>已报销 {formatMoney(transaction.reimbursed_amount, transaction.currency)}</span>
        )}
        {compact && <span>{formatDate(transaction.occurred_at)}</span>}
      </div>
      {!compact && hasReimburseActions && (
        <div className="row-actions">
          {reimbursable
            ? <>
                <button className="row-action reimburse" onClick={onReimburse} title="报销" aria-label="报销"><BadgeDollarSign size={16} /></button>
                <button className="row-action reimburse" onClick={onUnmarkReimbursable} title="取消待报销" aria-label="取消待报销"><X size={16} /></button>
              </>
            : (
                <button className="row-action reimburse" onClick={onMarkReimbursable} title="标记待报销" aria-label="标记待报销"><Tags size={16} /></button>
              )
          }
        </div>
      )}
      {!compact && (
        <button
          className="row-action"
          disabled={Boolean(transaction.voided_at) || transaction.kind === "loan"}
          onClick={onVoid}
          title="撤销并恢复余额"
          aria-label="撤销交易"
        ><Trash2 size={16} /></button>
      )}
    </div>
  );
}

export function InsightsPage({ summary, cashFlow }: { summary: MonthlySummary; cashFlow: CashFlowSummary }) {
  const gradient = buildDonutGradient(summary);
  return (
    <div className="page page-enter">
      <PageTitle eyebrow="INSIGHTS" title="收支分析" />
      <section className="insight-kpis">
        <SummaryCard label="本月收入" value={summary.total_income} currency={summary.currency} tone="green" />
        <SummaryCard label="本月支出" value={summary.total_expense} currency={summary.currency} tone="orange" />
        <SummaryCard label="本月结余" value={summary.net} currency={summary.currency} tone="blue" />
      </section>
      <CashFlowSankey summary={cashFlow} />
      <section className="insights-grid">
        <article className="panel donut-panel">
          <div className="section-heading compact-heading"><div><span>CATEGORY MIX</span><h2>分类占比</h2></div></div>
          <div className="donut-layout">
            <div className="donut" style={{ "--donut": gradient } as CSSProperties}>
              <div><span>总支出</span><strong>{formatMoney(summary.total_expense, summary.currency, true)}</strong></div>
            </div>
            <div className="donut-legend">
              {summary.expenses_by_category.map((item) => (
                <div key={item.category_id}><CategoryAvatar name={item.category_name} size="small" /><span>{item.category_name}</span><strong>{item.percentage}%</strong></div>
              ))}
            </div>
          </div>
        </article>
        <article className="panel insight-detail">
          <div className="section-heading compact-heading"><div><span>BREAKDOWN</span><h2>支出明细</h2></div></div>
          <CategoryBars summary={summary} detailed />
        </article>
      </section>
      <article className="insight-callout">
        <span className="callout-icon"><ChartNoAxesCombined size={22} /></span>
        <div><span>KOKU NOTE</span><h3>你保留了 {healthScore(summary)}% 的本月收入</h3></div>
      </article>
    </div>
  );
}

export interface SankeyDatum {
  id: string;
  name: string;
  amount: number;
  amountText: string;
  color: string;
}

export interface SankeyNode extends SankeyDatum {
  y: number;
  height: number;
}

export function MobileCashFlowGroup({
  label,
  nodes,
  total,
  currency
}: {
  label: string;
  nodes: SankeyNode[];
  total: number;
  currency: string;
}) {
  return (
    <section className="mobile-flow-group">
      <header><span>{label}</span><small>{nodes.length} 项</small></header>
      <div className="mobile-flow-list">
        {nodes.map((node) => (
          <article className="mobile-flow-item" key={node.id}>
            <div className="mobile-flow-meta">
              <span><i style={{ background: node.color }} />{node.name}</span>
              <strong>{formatMoney(node.amountText, currency)}</strong>
            </div>
            <div className="mobile-flow-track">
              <i
                style={{
                  width: `${Math.max(4, total > 0 ? (node.amount / total) * 100 : 0)}%`,
                  background: node.color
                }}
              />
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

export function CashFlowSankey({ summary }: { summary: CashFlowSummary }) {
  const layout = useMemo(() => {
    const retained = Number(summary.retained);
    const sources: SankeyDatum[] = summary.income_sources.map((item) => ({
      id: `income-${item.category_id}`,
      name: item.category_name,
      amount: Number(item.amount),
      amountText: item.amount,
      color: categoryVisual(item.category_name).color
    }));
    const destinations: SankeyDatum[] = summary.expense_destinations.map((item) => ({
      id: `expense-${item.category_id}`,
      name: item.category_name,
      amount: Number(item.amount),
      amountText: item.amount,
      color: categoryVisual(item.category_name).color
    }));
    if (retained < 0) {
      sources.push({
        id: "deficit",
        name: "动用存量资金",
        amount: Math.abs(retained),
        amountText: String(Math.abs(retained)),
        color: "#c27b58"
      });
    } else if (retained > 0) {
      destinations.push({
        id: "retained",
        name: "本月结余",
        amount: retained,
        amountText: summary.retained,
        color: "#3f9d70"
      });
    }

    const flowTotal = Number(summary.flow_total);
    const count = Math.max(sources.length, destinations.length, 1);
    const height = Math.max(430, count * 76 + 90);
    const top = 48;
    const bottom = 44;
    const gap = 20;
    const flowArea = height - top - bottom;
    const position = (items: SankeyDatum[]): SankeyNode[] => {
      if (!items.length || flowTotal <= 0) return [];
      const available = Math.max(40, flowArea - gap * Math.max(0, items.length - 1));
      const minimum = Math.min(7, available / items.length);
      const proportional = Math.max(0, available - minimum * items.length);
      let cursor = top;
      return items.map((item) => {
        const nodeHeight = minimum + (item.amount / flowTotal) * proportional;
        const node = { ...item, y: cursor, height: nodeHeight };
        cursor += nodeHeight + gap;
        return node;
      });
    };
    const sourceNodes = position(sources);
    const destinationNodes = position(destinations);
    const sourceHeight = sourceNodes.reduce((sum, item) => sum + item.height, 0);
    const destinationHeight = destinationNodes.reduce((sum, item) => sum + item.height, 0);
    const centerHeight = Math.max(sourceHeight, destinationHeight, 12);
    return {
      height,
      sources: sourceNodes,
      destinations: destinationNodes,
      centerY: (height - centerHeight) / 2,
      centerHeight,
      empty: flowTotal <= 0
    };
  }, [summary]);

  if (layout.empty) {
    return (
      <details className="panel cash-flow-panel" open>
        <summary><span><ChevronDown size={18} />现金流</span><small>收入如何流向支出</small></summary>
        <EmptyState title="暂无现金流" detail="记录本月收入或支出后，这里会生成资金流向图。" />
      </details>
    );
  }

  let sourceCenterCursor = layout.centerY;
  const sourceRibbons = layout.sources.map((node) => {
    const centerY = sourceCenterCursor;
    sourceCenterCursor += node.height;
    return { node, centerY };
  });
  let destinationCenterCursor = layout.centerY;
  const destinationRibbons = layout.destinations.map((node) => {
    const centerY = destinationCenterCursor;
    destinationCenterCursor += node.height;
    return { node, centerY };
  });

  return (
    <details className="panel cash-flow-panel" open>
      <summary>
        <span><ChevronDown size={18} />现金流</span>
      </summary>
      <div className="sankey-scroll">
        <svg
          className="sankey-canvas"
          viewBox={`0 0 1080 ${layout.height}`}
          role="img"
          aria-label={`${summary.month}月现金流向图`}
        >
          <title>{summary.month} 月现金流</title>
          <desc>左侧为收入分类，中间为本月现金流，右侧为支出分类与结余，连线宽度代表金额。</desc>
          {sourceRibbons.map(({ node, centerY }) => (
            <path
              key={`source-ribbon-${node.id}`}
              className="sankey-ribbon income-ribbon"
              d={sankeyRibbonPath(158, node.y, node.height, 522, centerY, node.height)}
              style={{ fill: node.color }}
            >
              <title>{node.name}：{formatMoney(node.amountText, summary.currency)}</title>
            </path>
          ))}
          {destinationRibbons.map(({ node, centerY }) => (
            <path
              key={`destination-ribbon-${node.id}`}
              className="sankey-ribbon expense-ribbon"
              d={sankeyRibbonPath(546, centerY, node.height, 930, node.y, node.height)}
              style={{ fill: node.color }}
            >
              <title>{node.name}：{formatMoney(node.amountText, summary.currency)}</title>
            </path>
          ))}

          {layout.sources.map((node) => (
            <g className="sankey-node" key={node.id}>
              <rect x="144" y={node.y} width="14" height={node.height} rx="5" style={{ fill: node.color }} />
              <text x="132" y={node.y + node.height / 2 - 2} textAnchor="end">
                <tspan className="node-name">{node.name}</tspan>
                <tspan className="node-amount" x="132" dy="17">{formatMoney(node.amountText, summary.currency)}</tspan>
              </text>
            </g>
          ))}

          <g className="sankey-center-node">
            <rect x="522" y={layout.centerY} width="24" height={layout.centerHeight} rx="5" />
            <text x="558" y={layout.centerY + layout.centerHeight / 2 - 3}>
              <tspan className="center-name">本月现金流</tspan>
              <tspan className="center-amount" x="558" dy="20">{formatMoney(summary.flow_total, summary.currency)}</tspan>
            </text>
          </g>

          {layout.destinations.map((node) => (
            <g className="sankey-node" key={node.id}>
              <rect x="930" y={node.y} width="14" height={node.height} rx="5" style={{ fill: node.color }} />
              <text x="958" y={node.y + node.height / 2 - 2}>
                <tspan className="node-name">{node.name}</tspan>
                <tspan className="node-amount" x="958" dy="17">{formatMoney(node.amountText, summary.currency)}</tspan>
              </text>
            </g>
          ))}
        </svg>
      </div>
      <div className="cash-flow-mobile" role="img" aria-label={`${summary.month}月现金流向明细`}>
        <MobileCashFlowGroup
          label="收入来源"
          nodes={layout.sources}
          total={Number(summary.flow_total)}
          currency={summary.currency}
        />
        <div className="mobile-flow-core">
          <span>汇入本月现金流</span>
          <strong>{formatMoney(summary.flow_total, summary.currency)}</strong>
          <ChevronDown size={17} />
        </div>
        <MobileCashFlowGroup
          label="支出去向与结余"
          nodes={layout.destinations}
          total={Number(summary.flow_total)}
          currency={summary.currency}
        />
      </div>
      <p className="sankey-caption">
        <ShieldCheck size={14} /> 转账已排除；带宽仅由当前币种下已确认的收入和支出决定。
      </p>
    </details>
  );
}

export function sankeyRibbonPath(
  sourceX: number,
  sourceY: number,
  sourceHeight: number,
  targetX: number,
  targetY: number,
  targetHeight: number
): string {
  const control = (targetX - sourceX) * 0.5;
  return [
    `M ${sourceX} ${sourceY}`,
    `C ${sourceX + control} ${sourceY}, ${targetX - control} ${targetY}, ${targetX} ${targetY}`,
    `L ${targetX} ${targetY + targetHeight}`,
    `C ${targetX - control} ${targetY + targetHeight}, ${sourceX + control} ${sourceY + sourceHeight}, ${sourceX} ${sourceY + sourceHeight}`,
    "Z"
  ].join(" ");
}

export function CategoryBars({ summary, detailed = false }: { summary: MonthlySummary; detailed?: boolean }) {
  if (!summary.expenses_by_category.length) return <EmptyState title="暂无支出数据" detail="记录支出后会自动生成分类分析。" />;
  return (
    <div className={`category-bars ${detailed ? "detailed" : ""}`}>
      {summary.expenses_by_category.slice(0, detailed ? 8 : 4).map((item) => (
        <div className="category-bar" key={item.category_id}>
          <div><span><CategoryAvatar name={item.category_name} size="small" />{item.category_name}</span><strong>{formatMoney(item.amount, summary.currency)}</strong></div>
          <div className="bar-track"><i style={{ width: `${item.percentage}%`, background: categoryVisual(item.category_name).color }} /></div>
          {detailed && <small>{item.percentage}% 的本月支出</small>}
        </div>
      ))}
    </div>
  );
}

export function LoansSection({
  loans,
  accounts,
  onCreateLoan,
  onRepay
}: {
  loans: Loan[];
  accounts: Account[];
  onCreateLoan: () => void;
  onRepay: (loan: Loan) => void;
}) {
  const accountMap = useMemo(() => new Map(accounts.map((account) => [account.id, account])), [accounts]);
  const open = loans.filter((loan) => !loan.closed_at);
  const closed = loans.filter((loan) => loan.closed_at);
  const lendOutstanding = open
    .filter((loan) => loan.loan_type === "lend")
    .reduce((sum, loan) => sum + Number(loan.outstanding), 0);
  const borrowOutstanding = open
    .filter((loan) => loan.loan_type === "borrow")
    .reduce((sum, loan) => sum + Number(loan.outstanding), 0);
  const currency = open[0]?.currency ?? "CNY";
  return (
    <section className="section-block account-group">
      <div className="section-heading compact-heading">
        <div><span>LOANS</span><h2>借入与借出</h2></div>
        <button className="text-button" onClick={onCreateLoan}><Plus size={16} /> 记一笔借款</button>
      </div>
      <div className="balance-summary-row">
        <SummaryCard label="借出应收" value={lendOutstanding.toFixed(2)} currency={currency} tone="green" />
        <SummaryCard label="借入应付" value={borrowOutstanding.toFixed(2)} currency={currency} tone="orange" />
      </div>
      <div className="account-grid">
        {open.map((loan) => (
          <article className="account-detail-card" key={loan.id}>
            <span className={`large-account-icon tone-${loan.id % 4}`}><Handshake size={23} /></span>
            <div className="account-detail-copy">
              <h3>
                {loan.counterparty}
                <small className={loan.loan_type === "lend" ? "income-text" : "expense-text"}>
                  {loan.loan_type === "lend" ? "借出" : "借入"}
                </small>
              </h3>
              <span>
                {loan.currency} · 本金 {formatMoney(loan.principal, loan.currency)} ·{" "}
                {accountMap.get(loan.account_id)?.name ?? "未知账户"}
                {loan.note ? ` · ${loan.note}` : ""}
              </span>
            </div>
            <strong>{formatMoney(loan.outstanding, loan.currency)}</strong>
            <button className="row-action" onClick={() => onRepay(loan)} title="还款" aria-label="还款"><RefreshCcw size={16} /></button>
          </article>
        ))}
        {open.length === 0 && <EmptyState title="没有进行中的借款" detail="点击“记一笔借款”借出或借入。" />}
      </div>
      {closed.length > 0 && (
        <div className="account-grid closed-loans">
          {closed.map((loan) => (
            <article className="account-detail-card muted" key={loan.id}>
              <span className="large-account-icon"><Handshake size={23} /></span>
              <div className="account-detail-copy">
                <h3>{loan.counterparty}<small>{loan.loan_type === "lend" ? "借出" : "借入"}</small></h3>
                <span>{formatDate(loan.opened_at)} 开立{loan.closed_at ? ` · ${formatDate(loan.closed_at)} 结清` : ""}</span>
              </div>
              <strong>已结清</strong>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}

export function EmptyState({ title, detail }: { title: string; detail: string }) {
  return <div className="empty-state"><span><ReceiptText size={20} /></span><div><strong>{title}</strong><p>{detail}</p></div></div>;
}
