//! 页面级组件：总览、账户、交易、分析、借贷区块。
import { useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowDownLeft,
  ArrowLeftRight,
  ArrowUpRight,
  BadgeDollarSign,
  Banknote,
  BellRing,
  ChartNoAxesCombined,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  CircleCheck,
  CircleDollarSign,
  CreditCard,
  Download,
  Eye,
  EyeOff,
  Handshake,
  LayoutDashboard,
  LoaderCircle,
  MoreHorizontal,
  Paperclip,
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
  Upload,
  WalletCards,
  X,
  type LucideIcon
} from "lucide-react";
import { buildDonutGradient, categoryVisual, formatDate, formatMoney, healthScore } from "../lib";
import { exportTransactions, loadRollingSummary, loadTagSummary, loadTrend, loadYearlySummary, rateHint, receiptUrl } from "../api";
import { CategoryAvatar } from "./avatar";
import type {
  Account,
  AccountType,
  AppData,
  Budget,
  CashFlowItem,
  CashFlowSummary,
  Category,
  CategoryKind,
  Deposit,
  Holding,
  Loan,
  MonthlySummary,
  MonthlyTrendPoint,
  RecurringRule,
  RollingSummary,
  Tag,
  TagSummary,
  Transaction,
  TransactionKind,
  YearlySummary
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

/** 把金额从原币种折算到显示币种：同币种或缺失汇率时返回 null（调用方回退原币显示）。 */
function convertedMoney(
  value: string,
  from: string,
  display: string,
  rates: Record<string, number> | undefined
): { amount: string; currency: string } | null {
  if (from === display) return null;
  const factor = rates?.[from];
  if (factor == null) return null;
  return { amount: (Number(value) * factor).toFixed(2), currency: display };
}

/** 拉取一批币种到显示币种的折算汇率：currency → 1 unit = factor display。 */
function useConversionRates(currencies: string[], display: string) {
  const [rates, setRates] = useState<Record<string, number>>({});
  const key = currencies.join("|");
  useEffect(() => {
    const needed = [...new Set(key ? key.split("|") : [])].filter((currency) => currency !== display);
    if (needed.length === 0) {
      setRates({});
      return;
    }
    let cancelled = false;
    Promise.all(
      needed.map(async (currency) => {
        try {
          const quote = await rateHint(currency, display);
          return [currency, Number(quote.rate)] as const;
        } catch {
          return null;
        }
      })
    ).then((pairs) => {
      if (!cancelled) {
        setRates(
          Object.fromEntries(
            pairs.filter((pair): pair is readonly [string, number] => pair != null)
          )
        );
      }
    });
    return () => {
      cancelled = true;
    };
  }, [key, display]);
  return rates;
}

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
          <TrendChart transactions={activeTransactions} currency={display} rates={rates} />
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

export function TrendChart({
  transactions,
  currency,
  rates = {}
}: {
  transactions: Transaction[];
  currency: string;
  /** 折算汇率表：交易币种 → 1 unit = factor display；缺汇率时跳过该币种。 */
  rates?: Record<string, number>;
}) {
  const points = useMemo(() => {
    const values = Array.from({ length: 12 }, (_, index) => ({ x: index, value: 0 }));
    for (const item of transactions) {
      if (item.kind === "transfer" || item.kind === "loan" || item.kind === "adjustment") continue;
      const factor = item.currency === currency ? 1 : rates[item.currency];
      if (factor == null) continue; // 无折算汇率时跳过该币种
      const day = new Date(item.occurred_at).getDate();
      const bucket = Math.min(11, Math.floor(((day - 1) / 31) * 12));
      const signed = item.kind === "income"
        ? Number(item.amount) * factor
        : -Number(item.amount) * factor;
      values[bucket].value += signed;
    }
    let running = 0;
    return values.map((item) => {
      running += item.value;
      return running;
    });
  }, [currency, transactions, rates]);
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
  return (
    <div className="trend-chart" aria-label={t("dashboard.trendAria")}>
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
      <div className="chart-labels"><span>{t("dashboard.monthStart")}</span><span>{t("dashboard.monthMiddle")}</span><span>{t("dashboard.today")}</span></div>
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

export function AccountsPage({
  data,
  onAddAccount,
  onEdit,
  onDeposit,
  onSettle,
  onCreateLoan,
  onRepay,
  onCreateRecurring,
  onDeleteRecurring,
  onBuyStock,
  onSellStock,
  onSetHoldingPrice,
  onReconcile,
  onRefreshHoldings,
  onRefreshHolding
}: {
  data: AppData;
  onAddAccount: () => void;
  onEdit: (account: Account) => void;
  onDeposit: (account: Account) => void;
  onSettle: (deposit: Deposit) => void;
  onCreateLoan: () => void;
  onRepay: (loan: Loan) => void;
  onCreateRecurring: () => void;
  onDeleteRecurring: (id: number) => void;
  onBuyStock: (symbol?: string) => void;
  onSellStock: (symbol: string) => void;
  onSetHoldingPrice: (holdingId: number, price: string) => void;
  /** 打开某账户的对账弹窗。 */
  onReconcile: (account: Account) => void;
  /** 刷新过期/缺失市价（懒拉取），透传给持仓区块。 */
  onRefreshHoldings: () => void;
  /** 强制刷新单只持仓市价（可选）。 */
  onRefreshHolding?: (holdingId: number) => void;
}) {
  const group = (type: AccountType) => data.accounts.filter((account) => account.account_type === type);
  const cash = group("cash");
  const savings = group("savings");
  const stock = group("stock");
  const credit = group("credit");
  const display = data.monthly.currency;
  const rateCurrencies = useMemo(
    () => [
      ...new Set([
        ...data.accounts.map((account) => account.currency),
        ...data.loans.map((loan) => loan.currency)
      ])
    ],
    [data.accounts, data.loans]
  );
  const rates = useConversionRates(rateCurrencies, display);
  const { t } = useTranslation();
  return (
    <div className="page page-enter">
      <PageTitle
        eyebrow="ACCOUNTS"
        title={t("nav.accounts")}
        actions={<button className="primary-button" onClick={onAddAccount}><Plus size={18} /> {t("accounts.newAccount")}</button>}
      />
      <section className="balance-summary-row">
        <SummaryCard label={t("accounts.totalAssets")} value={data.balance.total_assets} currency={data.balance.currency} tone="green" />
        <SummaryCard label={t("accounts.totalLiabilities")} value={data.balance.total_liabilities} currency={data.balance.currency} tone="orange" />
        <SummaryCard label={t("accounts.netWorth")} value={data.balance.net_worth} currency={data.balance.currency} tone="blue" />
      </section>
      <AccountGroup title={t("accounts.type.cash")} subtitle={t("accounts.accountCount", { count: cash.length })} accounts={cash} onEdit={onEdit} onReconcile={onReconcile} display={display} rates={rates} />
      <AccountGroup title={t("accounts.type.savings")} subtitle={t("accounts.accountCount", { count: savings.length })} accounts={savings} onEdit={onEdit} onReconcile={onReconcile} display={display} rates={rates} />
      <DepositSection
        deposits={data.deposits}
        accounts={data.accounts}
        display={display}
        rates={rates}
        onDeposit={onDeposit}
        onSettle={onSettle}
      />
      <AccountGroup title={t("accounts.type.stock")} subtitle={t("accounts.accountCount", { count: stock.length })} accounts={stock} onEdit={onEdit} onReconcile={onReconcile} display={display} rates={rates} />
      <AccountGroup title={t("accounts.type.credit")} subtitle={t("accounts.accountCount", { count: credit.length })} accounts={credit} onEdit={onEdit} onReconcile={onReconcile} display={display} rates={rates} />
      <LoansSection
        loans={data.loans}
        accounts={data.accounts}
        display={display}
        rates={rates}
        onCreateLoan={onCreateLoan}
        onRepay={onRepay}
      />
      <RecurringSection
        rules={data.recurring}
        accounts={data.accounts}
        categories={data.categories}
        onCreate={onCreateRecurring}
        onDelete={onDeleteRecurring}
      />
      <HoldingSection
        holdings={data.holdings}
        accounts={data.accounts}
        display={display}
        rates={rates}
        onBuy={onBuyStock}
        onSell={onSellStock}
        onSetPrice={onSetHoldingPrice}
        onRefreshHoldings={onRefreshHoldings}
        onRefreshHolding={onRefreshHolding}
      />
    </div>
  );
}

export function SummaryCard({ label, value, currency, tone }: { label: string; value: string; currency: string; tone: string }) {
  return (
    <article className={`summary-card ${tone}`}>
      <span>{label}</span>
      <strong>{formatMoney(value, currency)}</strong>
    </article>
  );
}

export function AccountGroup({
  title,
  subtitle,
  accounts,
  onEdit,
  onReconcile,
  display,
  rates
}: {
  title: string;
  subtitle: string;
  accounts: Account[];
  onEdit?: (account: Account) => void;
  /** 传入后账户卡片出现「对账」入口。 */
  onReconcile?: (account: Account) => void;
  /** 显示币种（右上角切换）；传入后余额/额度按汇率折算显示，并标注原币 */
  display: string;
  /** 折算汇率表：账户币种 → 1 unit = factor display */
  rates: Record<string, number>;
}) {
  const { t } = useTranslation();
  return (
    <section className="section-block account-group">
      <div className="section-heading compact-heading">
        <div><span>{subtitle}</span><h2>{title}</h2></div>
      </div>
      <div className="account-grid">
        {accounts.map((account, index) => {
          const Icon = accountIcon(account);
          const shown = convertedMoney(account.balance, account.currency, display, rates)
            ?? { amount: account.balance, currency: account.currency };
          const isConverted = shown.currency !== account.currency;
          const limitShown = account.credit_limit
            ? convertedMoney(account.credit_limit, account.currency, display, rates)
            : null;
          return (
            <article className="account-detail-card" key={account.id}>
              <span className={`large-account-icon tone-${index % 4}`}><Icon size={23} /></span>
              <div className="account-detail-copy">
                <h3>{account.name}</h3>
                <span>
                  {isConverted ? `${t("accounts.originalCurrency", { amount: formatMoney(account.balance, account.currency) })} · ` : ""}
                  {account.credit_limit
                    ? `${t("accounts.limitLabel")} ${formatMoney(limitShown?.amount ?? account.credit_limit, limitShown?.currency ?? account.currency)}`
                    : ""}
                </span>
              </div>
              <strong>{formatMoney(shown.amount, shown.currency)}</strong>
              <div className="account-card-actions">
                {onReconcile && <button className="text-button" onClick={() => onReconcile(account)}>{t("reconcile.start")}</button>}
                <button className="bare-button" aria-label={t("accounts.editAria", { name: account.name })} title={t("accounts.editTitle")} onClick={() => onEdit?.(account)}><MoreHorizontal size={19} /></button>              </div>
            </article>
          );
        })}
        {accounts.length === 0 && <EmptyState title={t("accounts.emptyTitle")} detail={t("accounts.emptyDetail")} />}
      </div>
    </section>
  );
}

/**
 * 标签多选下拉：可同时勾选多个标签（AND 语义），选中后标签筛选生效。
 */
function TagMultiSelect({
  tags,
  selected,
  onChange
}: {
  tags: Tag[];
  selected: string[];
  onChange: (next: string[]) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);
  const toggle = (name: string) => {
    onChange(selected.includes(name) ? selected.filter((item) => item !== name) : [...selected, name]);
  };
  const { t } = useTranslation();
  return (
    <div className="tag-multiselect" ref={ref}>
      <button
        type="button"
        className={`tag-filter-button ${selected.length > 0 ? "active" : ""}`}
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        aria-haspopup="listbox"
      >
        <Tags size={14} />
        {selected.length === 0 ? t("transactions.tagFilter") : selected.join(" + ")}
      </button>
      {open && (
        <div className="tag-multiselect-menu" role="listbox" aria-multiselectable="true">
          {tags.map((tag) => {
            const checked = selected.includes(tag.name);
            return (
              <label key={tag.id} className="tag-multiselect-option">
                <input type="checkbox" checked={checked} onChange={() => toggle(tag.name)} />
                {tag.name}
              </label>
            );
          })}
          {selected.length > 0 && (
            <button type="button" className="tag-multiselect-clear" onClick={() => onChange([])}>
              {t("transactions.clearTagFilter")}
            </button>
          )}
        </div>
      )}
    </div>
  );
}

export function TransactionsPage({
  data,
  onAdd,
  onImport,
  onVoid,
  onRestore,
  onDeletePermanently,
  onMarkReimbursable,
  onUnmarkReimbursable,
  onReimburse,
  onEdit,
  onUploadReceipt,
  onLoadMore,
  loadingMore = false,
  hasMore = false,
  exportYear,
  exportMonth
}: {
  data: AppData;
  onAdd: () => void;
  /** 打开批量导入弹窗。 */
  onImport: () => void;
  onVoid: (transaction: Transaction) => void;
  onRestore: (transaction: Transaction) => void;
  onDeletePermanently: (transaction: Transaction) => void;
  onMarkReimbursable: (transaction: Transaction) => void;
  onUnmarkReimbursable: (transaction: Transaction) => void;
  onReimburse: (transaction: Transaction) => void;
  onEdit: (transaction: Transaction) => void;
  onUploadReceipt: (transaction: Transaction, file: File) => void;
  onLoadMore?: () => void;
  loadingMore?: boolean;
  hasMore?: boolean;
  exportYear?: number;
  exportMonth?: number;
}) {
  const [search, setSearch] = useState("");
  const [kind, setKind] = useState<"all" | TransactionKind>("all");
  const [tagFilter, setTagFilter] = useState<string[]>([]);
  const [tagSummary, setTagSummary] = useState<TagSummary | null>(null);
  const [tagSummaryError, setTagSummaryError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [exportError, setExportError] = useState<string | null>(null);
  const { t } = useTranslation();
  const handleExport = async () => {
    setExporting(true);
    setExportError(null);
    try {
      await exportTransactions(exportYear, exportMonth);
    } catch (reason) {
      setExportError(reason instanceof Error ? reason.message : t("transactions.exportLoadFailed"));
    } finally {
      setExporting(false);
    }
  };
  const accountsById = useMemo(() => new Map(data.accounts.map((item) => [item.id, item])), [data.accounts]);
  const categoriesById = useMemo(() => new Map(data.categories.map((item) => [item.id, item])), [data.categories]);
  const display = data.monthly.currency;
  const txCurrencies = useMemo(
    () => [...new Set(data.transactions.map((item) => item.currency))],
    [data.transactions]
  );
  const rates = useConversionRates(txCurrencies, display);
  const tagFilterKey = tagFilter.join(",");
  // 选中标签时拉取对应汇总（月视图按当前月，全部月份视图按全部历史）。
  useEffect(() => {
    if (tagFilter.length === 0) {
      setTagSummary(null);
      setTagSummaryError(null);
      return;
    }
    let cancelled = false;
    setTagSummaryError(null);
    loadTagSummary(tagFilter, display, exportYear, exportMonth)
      .then((summary) => {
        if (!cancelled) setTagSummary(summary);
      })
      .catch((reason) => {
        if (!cancelled) {
          setTagSummary(null);
          setTagSummaryError(reason instanceof Error ? reason.message : t("transactions.tagSummaryLoadFailed"));
        }
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tagFilterKey, display, exportYear, exportMonth]);
  const filtered = data.transactions.filter((item) => {
    const category = item.category_id ? categoriesById.get(item.category_id)?.name ?? "" : "转账";
    const account = accountsById.get(item.account_id)?.name ?? "";
    const matchesSearch = `${item.note} ${category} ${account}`.toLowerCase().includes(search.toLowerCase());
    const matchesTag = tagFilter.length === 0 || tagFilter.every((name) => item.tags.includes(name));
    return matchesSearch && matchesTag && (kind === "all" || item.kind === kind);
  });
  return (
    <div className="page page-enter">
      <PageTitle
        eyebrow="TRANSACTIONS"
        title={t("transactions.title")}
        actions={<button className="primary-button" onClick={onAdd}><Plus size={18} /> {t("common.quickAdd")}</button>}
      />
      <div className="transaction-toolbar">
        <label className="search-box"><Search size={18} /><input value={search} onChange={(e) => setSearch(e.target.value)} placeholder={t("transactions.searchPlaceholder")} /></label>
        <div className="segmented-filter">
          {(["all", "expense", "income", "transfer", "loan"] as const).map((item) => (
            <button key={item} className={kind === item ? "active" : ""} onClick={() => setKind(item)}>
              {t(`transactions.kind.${item}`)}
            </button>
          ))}
        </div>
        {data.tags.length > 0 && (
          <TagMultiSelect
            tags={data.tags}
            selected={tagFilter}
            onChange={setTagFilter}
          />
        )}
        <button
          type="button"
          className="text-button export-button"
          onClick={() => void handleExport()}
          disabled={exporting}
          title={exportYear !== undefined && exportMonth !== undefined ? t("transactions.exportTitle", { year: exportYear, month: exportMonth }) : t("transactions.exportAll")}
        >
          {exporting ? <LoaderCircle className="spin" size={16} /> : <Download size={16} />}
          {exporting ? t("transactions.exporting") : t("transactions.exportCsv")}
        </button>
        <button
          type="button"
          className="text-button import-button"
          onClick={onImport}
          title={t("transactions.importTitle")}
        >
          <Upload size={16} /> {t("transactions.import")}
        </button>
      </div>
      {tagFilter.length > 0 && (
        <section className="tag-summary" aria-label={t("transactions.tagSummaryAria")}>
          {tagSummaryError ? (
            <span className="inline-error">{t("transactions.tagSummaryError")}{tagSummaryError}</span>
          ) : tagSummary ? (
            <>
              <div className="tag-summary-total">
                <span className="tag-summary-label">
                  {t("transactions.tagSummary.tags", { tags: tagSummary.tags.join(" + ") })}
                  {tagSummary.year ? t("transactions.tagSummary.period", { year: tagSummary.year, month: tagSummary.month }) : t("transactions.tagSummary.allHistory")}
                  {t("transactions.tagSummary.total")}
                </span>
                <strong>{t("common.expense")} {formatMoney(tagSummary.total_expense, tagSummary.currency)}</strong>
                <span>{t("common.income")} {formatMoney(tagSummary.total_income, tagSummary.currency)}</span>
                <span className={Number(tagSummary.retained) >= 0 ? "positive" : "negative"}>
                  {t("common.net")} {formatMoney(tagSummary.retained, tagSummary.currency)}
                </span>
              </div>
              {tagSummary.expense_destinations.length > 0 && (
                <div className="tag-summary-breakdown">
                  {tagSummary.expense_destinations.map((item) => (
                    <span key={item.category_id} className="tag-summary-item">
                      {item.category_name} {formatMoney(item.amount, tagSummary.currency)}
                      <em>{item.percentage}%</em>
                    </span>
                  ))}
                </div>
              )}
            </>
          ) : (
            <span className="inline-error">{t("transactions.tagSummaryLoading")}</span>
          )}
        </section>
      )}
      {exportError && <div className="inline-error">{t("transactions.exportFailed")}{exportError}</div>}
      <article className="panel transaction-table">
        <div className="table-header"><span>{t("transactions.colTransaction")}</span><span>{t("transactions.colAccount")}</span><span>{t("transactions.colDate")}</span><span>{t("transactions.colAmount")}</span><span /><span /></div>
        {filtered.map((transaction) => (
          <TransactionRow
            key={transaction.id}
            transaction={transaction}
            account={accountsById.get(transaction.account_id)}
            target={transaction.to_account_id ? accountsById.get(transaction.to_account_id) : undefined}
            category={transaction.category_id ? categoriesById.get(transaction.category_id) : undefined}
            display={display}
            rates={rates}
            onVoid={() => onVoid(transaction)}
            onRestore={() => onRestore(transaction)}
            onDeletePermanently={() => onDeletePermanently(transaction)}
            onMarkReimbursable={() => onMarkReimbursable(transaction)}
            onUnmarkReimbursable={() => onUnmarkReimbursable(transaction)}
            onReimburse={() => onReimburse(transaction)}
            onEdit={() => onEdit(transaction)}
            onUploadReceipt={(file) => onUploadReceipt(transaction, file)}
          />
        ))}
        {filtered.length === 0 && <EmptyState title={t("transactions.notFoundTitle")} detail={t("transactions.notFoundDetail")} />}
      </article>
      {hasMore && (
        <div className="load-more-row">
          <button
            type="button"
            className="text-button load-more-button"
            onClick={onLoadMore}
            disabled={loadingMore}
          >
            {loadingMore
              ? <><LoaderCircle className="spin" size={16} /> {t("common.loading")}</>
              : t("transactions.loadMore", { count: data.transactions.length })}
          </button>
        </div>
      )}
    </div>
  );
}

export function TransactionList({
  transactions,
  accounts,
  categories,
  display,
  rates
}: {
  transactions: Transaction[];
  accounts: Account[];
  categories: Category[];
  /** 显示币种（右上角切换）；传入后金额按汇率折算显示 */
  display?: string;
  /** 折算汇率表：交易币种 → 1 unit = factor display */
  rates?: Record<string, number>;
}) {
  const accountMap = new Map(accounts.map((item) => [item.id, item]));
  const categoryMap = new Map(categories.map((item) => [item.id, item]));
  const { t } = useTranslation();
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
          display={display}
          rates={rates}
        />
      ))}
      {transactions.length === 0 && <EmptyState title={t("transactions.emptyTitle")} detail={t("transactions.emptyDetail")} />}
    </div>
  );
}

export function TransactionRow({
  transaction,
  account,
  target,
  category,
  compact = false,
  display,
  rates,
  onVoid,
  onRestore,
  onDeletePermanently,
  onMarkReimbursable,
  onUnmarkReimbursable,
  onReimburse,
  onEdit,
  onUploadReceipt
}: {
  transaction: Transaction;
  account?: Account;
  target?: Account;
  category?: Category;
  compact?: boolean;
  /** 显示币种（右上角切换）；传入后金额按汇率折算显示，原币金额保留为辅助行 */
  display?: string;
  /** 折算汇率表：交易币种 → 1 unit = factor display；缺汇率的币种回退原币显示 */
  rates?: Record<string, number>;
  onVoid?: () => void;
  /** 已撤销的流水：恢复（撤销删除） */
  onRestore?: () => void;
  /** 已撤销的流水：永久删除（不可恢复） */
  onDeletePermanently?: () => void;
  onMarkReimbursable?: () => void;
  onUnmarkReimbursable?: () => void;
  onReimburse?: () => void;
  /** 传入后在行最右侧显示 ⋯ 菜单（编辑交易） */
  onEdit?: () => void;
  /** 传入后菜单里出现「上传小票」 */
  onUploadReceipt?: (file: File) => void;
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const fileRef = useRef<HTMLInputElement | null>(null);
  // 点击菜单外部时关闭。
  useEffect(() => {
    if (!menuOpen) return;
    const close = (event: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        setMenuOpen(false);
      }
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [menuOpen]);
  const { t } = useTranslation();
  const meta = {
    expense: { icon: ArrowUpRight, label: category?.name ?? t("transactions.kind.expense"), className: "expense" },
    income: { icon: ArrowDownLeft, label: category?.name ?? t("transactions.kind.income"), className: "income" },
    transfer: { icon: ArrowLeftRight, label: t("transactions.meta.transfer"), className: "transfer" },
    loan: { icon: Handshake, label: transaction.note || t("transactions.meta.loan"), className: "transfer" },
    adjustment: { icon: RotateCcw, label: t("transactions.meta.adjustment"), className: "transfer" },
    trade: { icon: TrendingUp, label: t("transactions.meta.trade"), className: "transfer" },
    deposit: { icon: PiggyBank, label: t("transactions.meta.deposit"), className: "transfer" }
  }[transaction.kind];
  const Icon = meta.icon;
  const prefix =
    transaction.kind === "expense" ? "−"
    : transaction.kind === "income" ? "+"
    : transaction.kind === "adjustment" || transaction.kind === "trade" || transaction.kind === "deposit"
      ? (Number(transaction.amount) > 0 ? "+" : "")
    : "";
  const reimbursable = transaction.reimbursable_at && !transaction.reimbursed_at;
  const hasReimburseActions = transaction.kind === "expense" && !transaction.voided_at && !transaction.reimbursed_at;

  // 折算显示：display 币种与交易币种不同且有汇率时，主金额换算为显示币种，
  // 并用一行「原币」保留真实入账金额；无汇率时回退原币显示。
  const factor = display && transaction.currency !== display ? rates?.[transaction.currency] : undefined;
  const converted = factor != null;
  const mainAmount = converted ? (Number(transaction.amount) * factor!).toFixed(2) : transaction.amount;
  const mainCurrency = converted ? display! : transaction.currency;
  const targetFactor =
    display && transaction.target_currency && transaction.target_currency !== display
      ? rates?.[transaction.target_currency]
      : undefined;
  const targetConverted = targetFactor != null && transaction.target_amount != null;
  const targetAmount = targetConverted
    ? (Number(transaction.target_amount) * targetFactor!).toFixed(2)
    : transaction.target_amount;
  const targetCurrency = targetConverted ? display! : transaction.target_currency;
  const reimbursedShown = converted
    ? formatMoney((Number(transaction.reimbursed_amount) * factor!).toFixed(2), display!)
    : formatMoney(transaction.reimbursed_amount, transaction.currency);
  return (
    <div className={`transaction-row ${compact ? "compact-row" : ""} ${transaction.voided_at ? "voided" : ""}`}>
      <div className="transaction-main">
        {transaction.kind === "transfer" || transaction.kind === "loan" || transaction.kind === "adjustment" || transaction.kind === "trade" || transaction.kind === "deposit" ? (
          <span className={`transaction-icon ${meta.className}`}><Icon size={18} /></span>
        ) : (
          <CategoryAvatar name={meta.label} className={`transaction-icon ${meta.className}`} />
        )}
        <div>
          <strong>
            {transaction.note || meta.label}
            {transaction.voided_at ? t("transactions.voidedSuffix") : ""}
          </strong>
          <span className="transaction-meta">
            <span>{meta.label}</span>
            {reimbursable ? <span className="reimburse-status">{t("transactions.pendingReimburse")}</span> : ""}
            {transaction.has_receipt ? <span className="receipt-status"><Paperclip size={11} /> {t("transactions.receipt")}</span> : ""}
            {transaction.tags.map((tag) => (
              <span className="transaction-tag" key={tag}>#{tag}</span>
            ))}
          </span>
        </div>
      </div>
      {!compact && <span className="table-account">{account?.name ?? t("common.unknownAccount")}{target ? ` → ${target.name}` : ""}</span>}
      {!compact && <span className="table-date">{formatDate(transaction.occurred_at)}</span>}
      <div className={`transaction-amount ${meta.className}`}>
        <strong>{prefix}{formatMoney(mainAmount, mainCurrency)}</strong>
        {converted && <span>{t("transactions.original")} {formatMoney(transaction.amount, transaction.currency)}</span>}
        {transaction.kind === "transfer" && targetAmount && targetCurrency && (
          <span>{t("transactions.arrived")} {formatMoney(targetAmount, targetCurrency)}</span>
        )}
        {!converted && transaction.kind !== "transfer" && transaction.kind !== "loan" && transaction.kind !== "adjustment" && account && transaction.currency !== account.currency && (
          <span>{t("transactions.settledLabel")} {formatMoney(transaction.settled_amount, account.currency)}</span>
        )}
        {transaction.kind === "expense" && transaction.reimbursed_amount !== "0" && !transaction.reimbursed_at && (
          <span>{t("transactions.reimbursedLabel")} {reimbursedShown}</span>
        )}
        {compact && <span>{formatDate(transaction.occurred_at)}</span>}
      </div>
      {!compact && (
        <div className="transaction-actions">
          {hasReimburseActions && (
            reimbursable
              ? <>
                  <button className="row-action reimburse" onClick={onReimburse} title={t("transactions.reimburse")} aria-label={t("transactions.reimburse")}><BadgeDollarSign size={16} /></button>
                  <button className="row-action reimburse" onClick={onUnmarkReimbursable} title={t("transactions.unmarkReimburse")} aria-label={t("transactions.unmarkReimburse")}><X size={16} /></button>
                </>
              : <button className="row-action reimburse" onClick={onMarkReimbursable} title={t("transactions.markReimburse")} aria-label={t("transactions.markReimburse")}><Tags size={16} /></button>
          )}
          {transaction.reimbursed_at && (
            <span
              className="reimbursed-indicator"
              title={t("transactions.reimbursedTitle", { amount: reimbursedShown })}
              aria-label={t("transactions.reimbursedTitle", { amount: reimbursedShown })}
            ><CircleCheck size={16} /></span>
          )}
          {transaction.voided_at
            ? onRestore && (
                <button
                  className="row-action"
                  onClick={onRestore}
                  title={t("transactions.restoreTitle")}
                  aria-label={t("transactions.restore")}
                ><RotateCcw size={16} /></button>
              )
            : (
                <button
                  className="row-action"
                  disabled={transaction.kind === "loan" || transaction.kind === "trade" || transaction.kind === "deposit"}
                  onClick={onVoid}
                  title={t("transactions.voidTitle")}
                  aria-label={t("transactions.voidAria")}
                ><Trash2 size={16} /></button>
              )}
        </div>
      )}
      {(onEdit || onRestore || onDeletePermanently) && (
        <div className="row-menu-wrap" ref={menuRef}>
          <button
            type="button"
            className={`row-action ${menuOpen ? "active" : ""}`}
            onClick={() => setMenuOpen((open) => !open)}
            title={t("transactions.moreActions")}
            aria-label={t("transactions.moreActions")}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
          ><MoreHorizontal size={16} /></button>
          {menuOpen && (
            <div className="row-menu" role="menu">
              {transaction.voided_at ? (
                <>
                  {onRestore && (
                    <button type="button" role="menuitem" onClick={() => { setMenuOpen(false); onRestore(); }}>
                      {t("transactions.restore")}
                    </button>
                  )}
                  {onDeletePermanently && (
                    <button
                      type="button"
                      role="menuitem"
                      className="menu-danger"
                      onClick={() => { setMenuOpen(false); onDeletePermanently(); }}
                    >
                      {t("transactions.deletePermanent")}
                    </button>
                  )}
                  {transaction.has_receipt && (
                    <button
                      type="button"
                      role="menuitem"
                      onClick={() => {
                        setMenuOpen(false);
                        window.open(receiptUrl(transaction.id), "_blank", "noopener");
                      }}
                    >
                      {t("transactions.viewReceipt")}
                    </button>
                  )}
                </>
              ) : (
                <>
                  {onEdit && (
                    <button type="button" role="menuitem" onClick={() => { setMenuOpen(false); onEdit(); }}>
                      {t("transactions.edit")}
                    </button>
                  )}
                  {onUploadReceipt && (
                    <>
                      <button type="button" role="menuitem" onClick={() => fileRef.current?.click()}>
                        {t("transactions.uploadReceipt")}
                      </button>
                      {transaction.has_receipt && (
                        <button
                          type="button"
                          role="menuitem"
                          onClick={() => {
                            setMenuOpen(false);
                            window.open(receiptUrl(transaction.id), "_blank", "noopener");
                          }}
                        >
                          {t("transactions.viewReceipt")}
                        </button>
                      )}
                      <input
                        ref={fileRef}
                        type="file"
                        accept="image/*,application/pdf"
                        hidden
                        onChange={(event) => {
                          const file = event.target.files?.[0];
                          if (file) onUploadReceipt(file);
                          event.target.value = "";
                          setMenuOpen(false);
                        }}
                      />
                    </>
                  )}
                </>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export function InsightsPage({
  summary,
  cashFlow,
  categories,
  budgets,
  onSetBudget,
  onClearBudget
}: {
  summary: MonthlySummary;
  cashFlow: CashFlowSummary;
  categories: Category[];
  budgets: Budget[];
  onSetBudget: (categoryId: number, limit: string) => void;
  onClearBudget: (categoryId: number) => void;
}) {
  const gradient = buildDonutGradient(summary);
  const { t } = useTranslation();
  return (
    <div className="page page-enter">
      <PageTitle eyebrow="INSIGHTS" title={t("insights.title")} />
      <section className="insight-kpis">
        <SummaryCard label={t("insights.incomeLabel")} value={summary.total_income} currency={summary.currency} tone="green" />
        <SummaryCard label={t("insights.expenseLabel")} value={summary.total_expense} currency={summary.currency} tone="orange" />
        <SummaryCard label={t("insights.netLabel")} value={summary.net} currency={summary.currency} tone="blue" />
      </section>
      <MonthlyTrendPanel currency={summary.currency} />
      <YearlySummaryPanel currency={summary.currency} />
      <RollingSummaryPanel currency={summary.currency} />
      <CashFlowSankey summary={cashFlow} />
      <BudgetPanel
        summary={summary}
        categories={categories}
        budgets={budgets}
        onSetBudget={onSetBudget}
        onClearBudget={onClearBudget}
      />
      <section className="insights-grid">
        <article className="panel donut-panel">
          <div className="section-heading compact-heading"><div><span>CATEGORY MIX</span><h2>{t("insights.categoryMix")}</h2></div></div>
          <div className="donut-layout">
            <div className="donut" style={{ "--donut": gradient } as CSSProperties}>
              <div><span>{t("insights.totalExpense")}</span><strong>{formatMoney(summary.total_expense, summary.currency, true)}</strong></div>
            </div>
            <div className="donut-legend">
              {summary.expenses_by_category.map((item) => (
                <div key={item.category_id}><CategoryAvatar name={item.category_name} size="small" /><span>{item.category_name}</span><strong>{item.percentage}%</strong></div>
              ))}
            </div>
          </div>
        </article>
        <article className="panel insight-detail">
          <div className="section-heading compact-heading"><div><span>BREAKDOWN</span><h2>{t("insights.breakdown")}</h2></div></div>
          <CategoryBars summary={summary} detailed />
        </article>
      </section>
      <article className="insight-callout">
        <span className="callout-icon"><ChartNoAxesCombined size={22} /></span>
        <div><span>KOKU NOTE</span><h3>{t("insights.retainedNote", { percent: healthScore(summary) })}</h3></div>
      </article>
    </div>
  );
}

export function MonthlyTrendPanel({ currency }: { currency: string }) {
  const [points, setPoints] = useState<MonthlyTrendPoint[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  useEffect(() => {
    let cancelled = false;
    loadTrend(12, currency)
      .then((data) => {
        if (!cancelled) {
          setPoints(data);
          setError(null);
        }
      })
      .catch((reason) => {
        if (!cancelled) setError(reason instanceof Error ? reason.message : t("insights.trendLoadFailed"));
      });
    return () => {
      cancelled = true;
    };
  }, [currency, t]);
  return (
    <section className="panel trend-panel">
      <div className="section-heading compact-heading">
        <div><span>TREND</span><h2>{t("insights.trendTitle")}</h2></div>
      </div>
      {error && <div className="trend-note">{t("insights.trendError")}{error}</div>}
      {!error && !points && <div className="trend-note">{t("common.loading")}</div>}
      {points && <MonthlyTrendChart points={points} currency={currency} />}
    </section>
  );
}

export function MonthlyTrendChart({ points, currency }: { points: MonthlyTrendPoint[]; currency: string }) {
  const { t } = useTranslation();
  if (points.length === 0) {
    return <EmptyState title={t("insights.trendEmptyTitle")} detail={t("insights.trendEmptyDetail")} />;
  }
  const width = 720;
  const height = 250;
  const padL = 54;
  const padR = 16;
  const padT = 18;
  const padB = 34;
  const incomes = points.map((point) => Number(point.total_income));
  const expenses = points.map((point) => Number(point.total_expense));
  const nets = points.map((point) => Number(point.net));
  const max = Math.max(1, ...incomes, ...expenses, ...nets);
  const min = Math.min(0, ...nets);
  const range = Math.max(1, max - min);
  const innerW = width - padL - padR;
  const innerH = height - padT - padB;
  const x = (index: number) =>
    padL + (points.length === 1 ? innerW / 2 : (index / (points.length - 1)) * innerW);
  const y = (value: number) => padT + ((max - value) / range) * innerH;
  const path = (values: number[]) =>
    values.map((value, index) => `${index ? "L" : "M"}${x(index).toFixed(1)},${y(value).toFixed(1)}`).join(" ");
  const gridLines = [0, 0.25, 0.5, 0.75, 1].map((fraction) => padT + fraction * innerH);
  return (
    <div className="monthly-trend-chart">
      <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label={t("insights.trendChartAria")}>
        <title>{t("insights.trendChartTitle", { currency })}</title>
        {gridLines.map((gy) => (
          <line key={gy} x1={padL} x2={width - padR} y1={gy} y2={gy} className="grid-line" />
        ))}
        <line x1={padL} x2={width - padR} y1={y(0)} y2={y(0)} className="trend-zero-line" />
        <path d={path(expenses)} className="trend-series expense" />
        <path d={path(incomes)} className="trend-series income" />
        <path d={path(nets)} className="trend-series net" />
        {points.map((point, index) => (
          <circle
            key={point.year * 100 + point.month}
            cx={x(index)}
            cy={y(Number(point.net))}
            r="3.2"
            className="trend-point"
          />
        ))}
        {points.map((point, index) => (
          <text
            key={`label-${point.year}-${point.month}`}
            x={x(index)}
            y={height - 10}
            textAnchor="middle"
            className="chart-axis-label"
          >
            {point.month === 1 ? t("insights.yearLabel", { year: point.year }) : t("insights.monthLabel", { month: point.month })}
          </text>
        ))}
      </svg>
      <div className="trend-legend">
        <span className="legend-income"><i />{t("common.income")}</span>
        <span className="legend-expense"><i />{t("common.expense")}</span>
        <span className="legend-net"><i />{t("common.net")}</span>
      </div>
    </div>
  );
}

/** 年度汇总面板：自加载指定年份的逐月收支、全年合计与分类明细。 */
export function YearlySummaryPanel({ currency }: { currency: string }) {
  const [year, setYear] = useState(() => new Date().getFullYear());
  const [summary, setSummary] = useState<YearlySummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const { t } = useTranslation();
  useEffect(() => {
    let cancelled = false;
    setSummary(null);
    setError(null);
    loadYearlySummary(year, currency)
      .then((data) => {
        if (!cancelled) {
          setSummary(data);
          setError(null);
        }
      })
      .catch((reason) => {
        if (!cancelled) setError(reason instanceof Error ? reason.message : t("insights.yearly.loadFailed"));
      });
    return () => {
      cancelled = true;
    };
  }, [year, currency, attempt, t]);
  return (
    <section className="panel trend-panel">
      <div className="section-heading compact-heading">
        <div><span>YEARLY</span><h2>{t("insights.yearly.title")}</h2></div>
        <div className="year-selector">
          <button type="button" className="icon-button" onClick={() => setYear((value) => value - 1)} aria-label={t("insights.yearly.prevYear")} title={t("insights.yearly.prevYear")}><ChevronLeft size={16} /></button>
          <strong>{t("insights.yearly.yearLabel", { year })}</strong>
          <button type="button" className="icon-button" onClick={() => setYear((value) => value + 1)} aria-label={t("insights.yearly.nextYear")} title={t("insights.yearly.nextYear")}><ChevronRight size={16} /></button>
        </div>
      </div>
      {error && (
        <div className="trend-note panel-error">
          <span>{t("insights.yearly.error")}{error}</span>
          <button type="button" className="text-button" onClick={() => setAttempt((value) => value + 1)}><RefreshCcw size={13} /> {t("common.retry")}</button>
        </div>
      )}
      {!error && !summary && <div className="trend-note">{t("common.loading")}</div>}
      {summary && (
        <>
          <div className="balance-summary-row insight-kpis">
            <SummaryCard label={t("insights.yearly.income")} value={summary.total_income} currency={summary.currency} tone="green" />
            <SummaryCard label={t("insights.yearly.expense")} value={summary.total_expense} currency={summary.currency} tone="orange" />
            <SummaryCard label={t("insights.yearly.net")} value={summary.net} currency={summary.currency} tone="blue" />
          </div>
          <YearlyBarChart summary={summary} />
          <div className="yearly-categories">
            <YearlyCategoryList title={t("common.incomeSources")} items={summary.income_sources} currency={summary.currency} />
            <YearlyCategoryList title={t("common.expenseDestinations")} items={summary.expense_destinations} currency={summary.currency} />
          </div>
        </>
      )}
    </section>
  );
}

/** 年度逐月收入/支出柱状图（12 个月，双色分组）。 */
function YearlyBarChart({ summary }: { summary: YearlySummary }) {
  const width = 720;
  const height = 200;
  const padL = 40;
  const padR = 12;
  const padT = 14;
  const padB = 28;
  const months = summary.months;
  const values = months.map((point) => [Number(point.total_income), Number(point.total_expense)] as const);
  const max = Math.max(1, ...values.flat());
  const innerW = width - padL - padR;
  const innerH = height - padT - padB;
  const slot = innerW / Math.max(1, months.length);
  const barWidth = Math.min(16, slot * 0.26);
  const y = (value: number) => padT + innerH - (value / max) * innerH;
  const barH = (value: number) => (value / max) * innerH;
  const gridLines = [0, 0.25, 0.5, 0.75, 1].map((fraction) => padT + fraction * innerH);
  const { t } = useTranslation();
  return (
    <div className="yearly-chart">
      <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label={t("insights.yearly.chartAria", { year: summary.year })}>
        <title>{t("insights.yearly.chartTitle", { year: summary.year, currency: summary.currency })}</title>
        {gridLines.map((gy) => (
          <line key={gy} x1={padL} x2={width - padR} y1={gy} y2={gy} className="grid-line" />
        ))}
        {months.map((point, index) => {
          const cx = padL + slot * index + slot / 2;
          const [income, expense] = values[index];
          return (
            <g key={point.year * 100 + point.month}>
              <rect x={cx - barWidth - 1.5} y={y(income)} width={barWidth} height={barH(income)} rx="2" className="yearly-bar income" />
              <rect x={cx + 1.5} y={y(expense)} width={barWidth} height={barH(expense)} rx="2" className="yearly-bar expense" />
              <text x={cx} y={height - 10} textAnchor="middle" className="chart-axis-label">
                {point.month === 1 ? t("insights.yearLabel", { year: point.year }) : t("insights.monthLabel", { month: point.month })}
              </text>
            </g>
          );
        })}
      </svg>
      <div className="trend-legend">
        <span className="legend-income"><i />{t("common.income")}</span>
        <span className="legend-expense"><i />{t("common.expense")}</span>
      </div>
    </div>
  );
}

/** 分类明细列表（收入来源 / 支出去向）。 */
function YearlyCategoryList({ title, items, currency }: { title: string; items: CashFlowItem[]; currency: string }) {
  if (items.length === 0) return null;
  return (
    <div className="yearly-category-group">
      <h3>{title}</h3>
      <div className="cashflow-list">
        {items.map((item) => (
          <div className="cashflow-item" key={item.category_id}>
            <CategoryAvatar name={item.category_name} size="small" />
            <span>{item.category_name}<em>{item.percentage}%</em></span>
            <strong>{formatMoney(item.amount, currency)}</strong>
          </div>
        ))}
      </div>
    </div>
  );
}

/** 滚动平均面板：自加载最近 N 个月的收支曲线 + trailing window 均值虚线。 */
export function RollingSummaryPanel({ currency }: { currency: string }) {
  const [monthsInput, setMonthsInput] = useState("12");
  const [windowInput, setWindowInput] = useState("3");
  const [summary, setSummary] = useState<RollingSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const { t } = useTranslation();
  const monthsSize = Number(monthsInput);
  const windowSize = Number(windowInput);
  const monthsValid = Number.isInteger(monthsSize) && monthsSize >= 1 && monthsSize <= 120;
  const windowValid = Number.isInteger(windowSize) && windowSize >= 1 && windowSize <= 120;
  const valid = monthsValid && windowValid;
  // 平均窗口不能超过趋势月数（后端上限约束），超限时按 months 截断。
  const effectiveWindow = valid ? Math.min(windowSize, monthsSize) : 0;
  useEffect(() => {
    if (!valid) return;
    let cancelled = false;
    setError(null);
    loadRollingSummary(monthsSize, effectiveWindow, currency)
      .then((data) => {
        if (!cancelled) {
          setSummary(data);
          setError(null);
        }
      })
      .catch((reason) => {
        if (!cancelled) setError(reason instanceof Error ? reason.message : t("insights.rolling.loadFailed"));
      });
    return () => {
      cancelled = true;
    };
  }, [monthsSize, effectiveWindow, currency, valid, attempt, t]);
  const windowClamped = windowValid && windowSize > monthsSize;
  return (
    <section className="panel trend-panel">
      <div className="section-heading compact-heading">
        <div><span>ROLLING AVG</span><h2>{t("insights.rolling.title")}</h2></div>
        <div className="rolling-controls">
          <label><span>{t("insights.rolling.months")}</span><input type="number" min={1} max={120} value={monthsInput} onChange={(e) => setMonthsInput(e.target.value)} aria-label={t("insights.rolling.monthsAria")} /></label>
          <label><span>{t("insights.rolling.window")}</span><input type="number" min={1} max={120} value={windowInput} onChange={(e) => setWindowInput(e.target.value)} aria-label={t("insights.rolling.windowAria")} /></label>
        </div>
      </div>
      {!valid && <div className="trend-note">{t("insights.rolling.invalid")}</div>}
      {windowClamped && <div className="trend-note">{t("insights.rolling.clamped", { months: monthsSize })}</div>}
      {error && (
        <div className="trend-note panel-error">
          <span>{t("insights.rolling.error")}{error}</span>
          <button type="button" className="text-button" onClick={() => setAttempt((value) => value + 1)}><RefreshCcw size={13} /> {t("common.retry")}</button>
        </div>
      )}
      {valid && !error && !summary && <div className="trend-note">{t("common.loading")}</div>}
      {valid && summary && (
        <>
          <RollingChart summary={summary} />
          <div className="trend-legend rolling-legend">
            <span className="legend-income"><i />{t("common.income")}</span>
            <span className="legend-expense"><i />{t("common.expense")}</span>
            <span className="legend-income avg"><i />{t("insights.rolling.incomeAvg")}</span>
            <span className="legend-expense avg"><i />{t("insights.rolling.expenseAvg")}</span>
          </div>
          <p className="rolling-hint">{t("insights.rolling.hint", { window: summary.window, currency: summary.currency })}</p>
        </>
      )}
    </section>
  );
}

/** 滚动平均曲线：收入/支出实线 + 各自 trailing avg 虚线。 */
function RollingChart({ summary }: { summary: RollingSummary }) {
  const points = summary.points;
  const { t } = useTranslation();
  if (points.length === 0) {
    return <EmptyState title={t("insights.rolling.emptyTitle")} detail={t("insights.rolling.emptyDetail")} />;
  }
  const width = 720;
  const height = 250;
  const padL = 54;
  const padR = 16;
  const padT = 18;
  const padB = 34;
  const incomes = points.map((point) => Number(point.income));
  const expenses = points.map((point) => Number(point.expense));
  const incomeAvgs = points.map((point) => Number(point.income_avg));
  const expenseAvgs = points.map((point) => Number(point.expense_avg));
  const max = Math.max(1, ...incomes, ...expenses, ...incomeAvgs, ...expenseAvgs);
  const innerW = width - padL - padR;
  const innerH = height - padT - padB;
  const x = (index: number) =>
    padL + (points.length === 1 ? innerW / 2 : (index / (points.length - 1)) * innerW);
  const y = (value: number) => padT + ((max - value) / max) * innerH;
  const path = (values: number[]) =>
    values.map((value, index) => `${index ? "L" : "M"}${x(index).toFixed(1)},${y(value).toFixed(1)}`).join(" ");
  const gridLines = [0, 0.25, 0.5, 0.75, 1].map((fraction) => padT + fraction * innerH);
  const labelEvery = Math.max(1, Math.ceil(points.length / 12));
  return (
    <div className="monthly-trend-chart">
      <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label={t("insights.rolling.chartAria")}>
        <title>{t("insights.rolling.chartTitle", { months: summary.months, window: summary.window, currency: summary.currency })}</title>
        {gridLines.map((gy) => (
          <line key={gy} x1={padL} x2={width - padR} y1={gy} y2={gy} className="grid-line" />
        ))}
        <path d={path(expenses)} className="trend-series expense" />
        <path d={path(incomes)} className="trend-series income" />
        <path d={path(expenseAvgs)} className="trend-series expense avg" />
        <path d={path(incomeAvgs)} className="trend-series income avg" />
        {points.map((point, index) => (
          <text
            key={`label-${point.year}-${point.month}`}
            x={x(index)}
            y={height - 10}
            textAnchor="middle"
            className="chart-axis-label"
          >
            {index % labelEvery === 0 ? (point.month === 1 ? t("insights.yearLabel", { year: point.year }) : t("insights.monthLabel", { month: point.month })) : ""}
          </text>
        ))}
      </svg>
    </div>
  );
}

export function BudgetPanel({
  summary,
  categories,
  budgets,
  onSetBudget,
  onClearBudget
}: {
  summary: MonthlySummary;
  categories: Category[];
  budgets: Budget[];
  onSetBudget: (categoryId: number, limit: string) => void;
  onClearBudget: (categoryId: number) => void;
}) {
  const [editingId, setEditingId] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const expenseCategories = useMemo(
    () => categories.filter((category) => category.kind === "expense"),
    [categories]
  );
  const actualByCategory = useMemo(
    () => new Map(summary.expenses_by_category.map((item) => [item.category_id, Number(item.amount)])),
    [summary.expenses_by_category]
  );
  const limitByCategory = useMemo(
    () => new Map(budgets.map((budget) => [budget.category_id, budget.limit_amount])),
    [budgets]
  );
  const rows = expenseCategories.filter(
    (category) => actualByCategory.has(category.id) || limitByCategory.has(category.id)
  );
  const { t } = useTranslation();
  return (
    <section className="panel budget-panel">
      <div className="section-heading compact-heading">
        <div><span>BUDGET</span><h2>{t("insights.budget.title")}</h2></div>
        <small>{t("insights.budget.period", { year: summary.year, month: summary.month })}</small>
      </div>
      {rows.length === 0 ? (
        <EmptyState title={t("insights.budget.emptyTitle")} detail={t("insights.budget.emptyDetail")} />
      ) : (
        <div className="budget-list">
          {rows.map((category) => {
            const actual = actualByCategory.get(category.id) ?? 0;
            const limit = limitByCategory.get(category.id);
            const limitNumber = limit === undefined ? null : Number(limit);
            const over = limitNumber !== null && actual > limitNumber;
            const ratio = limitNumber !== null && limitNumber > 0 ? actual / limitNumber : 0;
            const editing = editingId === category.id;
            return (
              <div className={`budget-row ${over ? "over" : ""}`} key={category.id}>
                <div className="budget-row-head">
                  <span className="budget-category">
                    <CategoryAvatar name={category.name} size="small" />
                    {category.name}
                  </span>
                  {editing ? (
                    <span className="budget-edit">
                      <input
                        type="number"
                        min="0"
                        step="0.01"
                        value={draft}
                        onChange={(event) => setDraft(event.target.value)}
                        placeholder={t("insights.budget.placeholder")}
                        autoFocus
                      />
                      <button
                        type="button"
                        className="row-action"
                        title={t("insights.budget.save")}
                        aria-label={t("insights.budget.save")}
                        onClick={() => {
                          if (draft.trim()) onSetBudget(category.id, draft.trim());
                          setEditingId(null);
                        }}
                      ><Check size={16} /></button>
                      {limit !== undefined && (
                        <button
                          type="button"
                          className="row-action"
                          title={t("insights.budget.clear")}
                          aria-label={t("insights.budget.clear")}
                          onClick={() => {
                            onClearBudget(category.id);
                            setEditingId(null);
                          }}
                        ><Trash2 size={16} /></button>
                      )}
                      <button
                        type="button"
                        className="row-action"
                        title={t("common.cancel")}
                        aria-label={t("common.cancel")}
                        onClick={() => setEditingId(null)}
                      ><X size={16} /></button>
                    </span>
                  ) : (
                    <span className="budget-amount">
                      <strong>{formatMoney(String(actual), summary.currency)}</strong>
                      <span>{limitNumber === null ? ` / ${t("insights.budget.unset")}` : ` / ${formatMoney(limit!, summary.currency)}`}</span>
                      <button
                        type="button"
                        className="row-action"
                        title={t("insights.budget.set")}
                        aria-label={t("insights.budget.set")}
                        onClick={() => {
                          setDraft(limit ?? "");
                          setEditingId(category.id);
                        }}
                      ><MoreHorizontal size={16} /></button>
                    </span>
                  )}
                </div>
                <div className="bar-track budget-track">
                  <i
                    style={{
                      width: `${Math.min(100, ratio * 100)}%`,
                      background: over ? "var(--expense)" : categoryVisual(category.name).color
                    }}
                  />
                </div>
                {over && (
                  <small className="budget-over-note">
                    {t("insights.budget.over", { amount: formatMoney(String(actual - limitNumber!), summary.currency) })}
                  </small>
                )}
              </div>
            );
          })}
        </div>
      )}
    </section>
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
  const { t } = useTranslation();
  return (
    <section className="mobile-flow-group">
      <header><span>{label}</span><small>{t("common.countItems", { count: nodes.length })}</small></header>
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
  const { t } = useTranslation();
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
        name: t("insights.cashflow.deficitSource"),
        amount: Math.abs(retained),
        amountText: String(Math.abs(retained)),
        color: "#c27b58"
      });
    } else if (retained > 0) {
      destinations.push({
        id: "retained",
        name: t("insights.cashflow.retained"),
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
  }, [summary, t]);

  if (layout.empty) {
    return (
      <details className="panel cash-flow-panel" open>
        <summary><span><ChevronDown size={18} />{t("insights.cashflow.title")}</span><small>{t("insights.cashflow.subtitle")}</small></summary>
        <EmptyState title={t("insights.cashflow.emptyTitle")} detail={t("insights.cashflow.emptyDetail")} />
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
        <span><ChevronDown size={18} />{t("insights.cashflow.title")}</span>
      </summary>
      <div className="sankey-scroll">
        <svg
          className="sankey-canvas"
          viewBox={`0 0 1080 ${layout.height}`}
          role="img"
          aria-label={t("insights.cashflow.chartAria", { month: summary.month })}
        >
          <title>{t("insights.cashflow.chartTitle", { month: summary.month })}</title>
          <desc>{t("insights.cashflow.chartDesc")}</desc>
          {sourceRibbons.map(({ node, centerY }) => (
            <path
              key={`source-ribbon-${node.id}`}
              className="sankey-ribbon income-ribbon"
              d={sankeyRibbonPath(158, node.y, node.height, 522, centerY, node.height)}
              style={{ fill: node.color }}
            >
              <title>{t("insights.cashflow.ribbonTitle", { name: node.name, amount: formatMoney(node.amountText, summary.currency) })}</title>
            </path>
          ))}
          {destinationRibbons.map(({ node, centerY }) => (
            <path
              key={`destination-ribbon-${node.id}`}
              className="sankey-ribbon expense-ribbon"
              d={sankeyRibbonPath(546, centerY, node.height, 930, node.y, node.height)}
              style={{ fill: node.color }}
            >
              <title>{t("insights.cashflow.ribbonTitle", { name: node.name, amount: formatMoney(node.amountText, summary.currency) })}</title>
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
              <tspan className="center-name">{t("insights.cashflow.center")}</tspan>
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
      <div className="cash-flow-mobile" role="img" aria-label={t("insights.cashflow.mobileAria", { month: summary.month })}>
        <MobileCashFlowGroup
          label={t("common.incomeSources")}
          nodes={layout.sources}
          total={Number(summary.flow_total)}
          currency={summary.currency}
        />
        <div className="mobile-flow-core">
          <span>{t("insights.cashflow.mobileCore")}</span>
          <strong>{formatMoney(summary.flow_total, summary.currency)}</strong>
          <ChevronDown size={17} />
        </div>
        <MobileCashFlowGroup
          label={t("insights.cashflow.mobileDestinations")}
          nodes={layout.destinations}
          total={Number(summary.flow_total)}
          currency={summary.currency}
        />
      </div>
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
  const { t } = useTranslation();
  if (!summary.expenses_by_category.length) return <EmptyState title={t("insights.categoryBars.emptyTitle")} detail={t("insights.categoryBars.emptyDetail")} />;
  return (
    <div className={`category-bars ${detailed ? "detailed" : ""}`}>
      {summary.expenses_by_category.slice(0, detailed ? 8 : 4).map((item) => (
        <div className="category-bar" key={item.category_id}>
          <div><span><CategoryAvatar name={item.category_name} size="small" />{item.category_name}</span><strong>{formatMoney(item.amount, summary.currency)}</strong></div>
          <div className="bar-track"><i style={{ width: `${item.percentage}%`, background: categoryVisual(item.category_name).color }} /></div>
          {detailed && <small>{t("insights.categoryBars.percentLabel", { percent: item.percentage })}</small>}
        </div>
      ))}
    </div>
  );
}

export function LoansSection({
  loans,
  accounts,
  display,
  rates,
  onCreateLoan,
  onRepay
}: {
  loans: Loan[];
  accounts: Account[];
  /** 显示币种（右上角切换）；传入后本金/未结按汇率折算显示 */
  display: string;
  /** 折算汇率表：借款币种 → 1 unit = factor display */
  rates: Record<string, number>;
  onCreateLoan: () => void;
  onRepay: (loan: Loan) => void;
}) {
  const accountMap = useMemo(() => new Map(accounts.map((account) => [account.id, account])), [accounts]);
  const open = loans.filter((loan) => !loan.closed_at);
  const closed = loans.filter((loan) => loan.closed_at);
  const shown = (value: string, from: string) =>
    convertedMoney(value, from, display, rates) ?? { amount: value, currency: from };
  const lendOutstanding = open
    .filter((loan) => loan.loan_type === "lend")
    .reduce((sum, loan) => sum + Number(shown(loan.outstanding, loan.currency).amount), 0);
  const borrowOutstanding = open
    .filter((loan) => loan.loan_type === "borrow")
    .reduce((sum, loan) => sum + Number(shown(loan.outstanding, loan.currency).amount), 0);
  const { t } = useTranslation();
  return (
    <section className="section-block account-group">
      <div className="section-heading compact-heading">
        <div><span>LOANS</span><h2>{t("accounts.loans.title")}</h2></div>
        <button className="text-button" onClick={onCreateLoan}><Plus size={16} /> {t("accounts.loans.add")}</button>
      </div>
      <div className="balance-summary-row">
        <SummaryCard label={t("accounts.loans.receivable")} value={lendOutstanding.toFixed(2)} currency={display} tone="green" />
        <SummaryCard label={t("accounts.loans.payable")} value={borrowOutstanding.toFixed(2)} currency={display} tone="orange" />
      </div>
      <div className="account-grid">
        {open.map((loan) => {
          const outstandingShown = shown(loan.outstanding, loan.currency);
          const principalShown = shown(loan.principal, loan.currency);
          const isConverted = outstandingShown.currency !== loan.currency;
          return (
            <article className="account-detail-card" key={loan.id}>
              <span className={`large-account-icon tone-${loan.id % 4}`}><Handshake size={23} /></span>
              <div className="account-detail-copy">
                <h3>
                  {loan.counterparty}
                  <small className={loan.loan_type === "lend" ? "income-text" : "expense-text"}>
                    {t(loan.loan_type === "lend" ? "common.lend" : "common.borrow")}
                  </small>
                </h3>
                <span>
                  {loan.currency}
                  {isConverted ? `（${t("accounts.originalCurrency", { amount: formatMoney(loan.outstanding, loan.currency) })}）` : ""} · {t("accounts.loans.principal")}{" "}
                  {formatMoney(principalShown.amount, principalShown.currency)}
                  {isConverted ? `（${t("accounts.originalCurrency", { amount: formatMoney(loan.principal, loan.currency) })}）` : ""} ·{" "}
                  {accountMap.get(loan.account_id)?.name ?? t("common.unknownAccount")}
                  {loan.note ? ` · ${loan.note}` : ""}
                </span>
              </div>
              <strong>{formatMoney(outstandingShown.amount, outstandingShown.currency)}</strong>
              <button className="row-action" onClick={() => onRepay(loan)} title={t("accounts.loans.repay")} aria-label={t("accounts.loans.repay")}><RefreshCcw size={16} /></button>
            </article>
          );
        })}
        {open.length === 0 && <EmptyState title={t("accounts.loans.emptyTitle")} detail={t("accounts.loans.emptyDetail")} />}
      </div>
      {closed.length > 0 && (
        <div className="account-grid closed-loans">
          {closed.map((loan) => (
            <article className="account-detail-card muted" key={loan.id}>
              <span className="large-account-icon"><Handshake size={23} /></span>
              <div className="account-detail-copy">
                <h3>{loan.counterparty}<small>{t(loan.loan_type === "lend" ? "common.lend" : "common.borrow")}</small></h3>
                <span>{formatDate(loan.opened_at)} {t("common.opened")}{loan.closed_at ? ` · ${formatDate(loan.closed_at)} ${t("common.settled")}` : ""}</span>
              </div>
              <strong>{t("common.settledDone")}</strong>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}

export function RecurringSection({
  rules,
  accounts,
  categories,
  onCreate,
  onDelete
}: {
  rules: RecurringRule[];
  accounts: Account[];
  categories: Category[];
  onCreate: () => void;
  onDelete: (id: number) => void;
}) {
  const accountMap = useMemo(() => new Map(accounts.map((account) => [account.id, account])), [accounts]);
  const categoryMap = useMemo(() => new Map(categories.map((category) => [category.id, category])), [categories]);
  const { t } = useTranslation();
  return (
    <section className="section-block account-group">
      <div className="section-heading compact-heading">
        <div><span>RECURRING</span><h2>{t("accounts.recurring.title")}</h2></div>
        <button className="text-button" onClick={onCreate}><Plus size={16} /> {t("accounts.recurring.new")}</button>
      </div>
      <div className="account-grid">
        {rules.map((rule) => {
          const account = accountMap.get(rule.account_id);
          const category = categoryMap.get(rule.category_id);
          const isExpense = rule.kind === "expense";
          const Icon = isExpense ? ArrowUpRight : ArrowDownLeft;
          return (
            <article className="account-detail-card" key={rule.id}>
              <span className={`large-account-icon ${isExpense ? "tone-1" : "tone-2"}`}><Icon size={23} /></span>
              <div className="account-detail-copy">
                <h3>{rule.note || category?.name || t("accounts.recurring.title")}</h3>
                <span>
                  {category?.name ?? t("common.unknownCategory")} · {rule.frequency === "monthly" ? t("common.monthly") : t("common.weekly")} · {t("accounts.recurring.next")} {formatDate(rule.next_due_at)} · {account?.name ?? t("common.unknownAccount")}
                </span>
              </div>
              <strong className={isExpense ? "expense-text" : "income-text"}>
                {isExpense ? "−" : "+"}{formatMoney(rule.amount, account?.currency ?? "CNY")}
              </strong>
              <button className="row-action" onClick={() => onDelete(rule.id)} title={t("accounts.recurring.delete")} aria-label={t("accounts.recurring.delete")}><Trash2 size={16} /></button>
            </article>
          );
        })}
        {rules.length === 0 && <EmptyState title={t("accounts.recurring.emptyTitle")} detail={t("accounts.recurring.emptyDetail")} />}
      </div>
    </section>
  );
}

export function DepositSection({
  deposits,
  accounts,
  display,
  rates,
  onDeposit,
  onSettle
}: {
  deposits: Deposit[];
  accounts: Account[];
  display: string;
  rates: Record<string, number>;
  onDeposit: (account: Account) => void;
  onSettle: (deposit: Deposit) => void;
}) {
  const savings = accounts.filter((account) => account.account_type === "savings");
  const open = deposits.filter((deposit) => !deposit.settled_at);
  const closed = deposits.filter((deposit) => deposit.settled_at);
  const accountMap = useMemo(() => new Map(accounts.map((account) => [account.id, account])), [accounts]);
  const shown = (value: string, from: string) =>
    convertedMoney(value, from, display, rates) ?? { amount: value, currency: from };
  const { t } = useTranslation();
  return (
    <section className="section-block account-group">
      <div className="section-heading compact-heading">
        <div><span>DEPOSITS</span><h2>{t("deposit.title")}</h2></div>
        {savings.length > 0 && (
          <button className="text-button" onClick={() => onDeposit(savings[0])}><PiggyBank size={16} /> {t("deposit.convert")}</button>
        )}
      </div>
      <div className="account-grid">
        {open.map((deposit) => {
          const principal = shown(deposit.amount, deposit.currency);
          return (
            <article className="account-detail-card" key={deposit.id}>
              <span className="large-account-icon tone-1"><PiggyBank size={23} /></span>
              <div className="account-detail-copy">
                <h3>{t("deposit.term", { days: deposit.term_days })}</h3>
                <span>
                  {t("deposit.meta", { rate: deposit.rate, date: formatDate(deposit.maturity_at), account: accountMap.get(deposit.source_account_id)?.name ?? t("common.unknownAccount") })}
                </span>
              </div>
              <strong>{formatMoney(principal.amount, principal.currency)}</strong>
              <button className="row-action" onClick={() => onSettle(deposit)} title={t("deposit.settleTitle")} aria-label={t("deposit.settleAria")}><RotateCcw size={16} /></button>
            </article>
          );
        })}
        {open.length === 0 && <EmptyState title={t("deposit.emptyTitle")} detail={t("deposit.emptyDetail")} />}
      </div>
      {closed.length > 0 && (
        <div className="account-grid closed-loans">
          {closed.map((deposit) => (
            <article className="account-detail-card muted" key={deposit.id}>
              <span className="large-account-icon"><PiggyBank size={23} /></span>
              <div className="account-detail-copy">
                <h3>{t("deposit.term", { days: deposit.term_days })}</h3>
                <span>{formatDate(deposit.opened_at)} {t("common.opened")}{deposit.settled_at ? ` · ${formatDate(deposit.settled_at)} ${t("common.settled")}` : ""}</span>
              </div>
              <strong>{t("common.settledDone")}</strong>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}

export function HoldingSection({
  holdings,
  accounts,
  display,
  rates,
  onBuy,
  onSell,
  onSetPrice,
  onRefreshHoldings,
  onRefreshHolding
}: {
  holdings: Holding[];
  accounts: Account[];
  display: string;
  rates: Record<string, number>;
  onBuy: (symbol?: string) => void;
  onSell: (symbol: string) => void;
  onSetPrice: (holdingId: number, price: string) => void;
  /** 刷新过期/缺失市价。 */
  onRefreshHoldings: () => void;
  /** 强制刷新单只持仓市价（可选）。 */
  onRefreshHolding?: (holdingId: number) => void;
}) {
  const accountMap = useMemo(() => new Map(accounts.map((account) => [account.id, account])), [accounts]);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const { t } = useTranslation();
  // 挂载后懒拉取：只要存在从未刷新或超过 24 小时未刷新的持仓，就在后台刷新一次市价。
  const didAutoRefresh = useRef(false);
  useEffect(() => {
    if (didAutoRefresh.current) return;
    const stale = holdings.some((holding) => {
      if (!holding.updated_at) return true;
      return Date.now() - Date.parse(holding.updated_at) > 24 * 3600 * 1000;
    });
    if (!stale) return;
    didAutoRefresh.current = true;
    Promise.resolve()
      .then(() => onRefreshHoldings())
      .catch(() => undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [holdings]);
  return (
    <section className="section-block account-group">
      <div className="section-heading compact-heading">
        <div><span>HOLDINGS</span><h2>{t("holdings.title")}</h2></div>
        <div className="holding-actions">
          <button className="text-button" onClick={() => void onRefreshHoldings()} title={t("holdings.refreshStaleTitle")} aria-label={t("holdings.refreshAria")}><RefreshCcw size={14} /> {t("holdings.refresh")}</button>
          <button className="text-button" onClick={() => onBuy()}><Plus size={16} /> {t("holdings.buy")}</button>
        </div>
      </div>
      <div className="account-grid">
        {holdings.map((holding) => {
          const account = accountMap.get(holding.account_id);
          const currency = account?.currency ?? "CNY";
          const shares = Number(holding.shares);
          const lastPrice = holding.last_price !== null ? Number(holding.last_price) : null;
          const marketValue = shares * (lastPrice ?? Number(holding.average_cost));
          const shown = convertedMoney(String(marketValue), currency, display, rates)
            ?? { amount: String(marketValue), currency };
          const editing = editingId === holding.id;
          return (
            <article className="account-detail-card" key={holding.id}>
              <span className="large-account-icon tone-3"><TrendingUp size={23} /></span>
              <div className="account-detail-copy">
                <h3>{holding.symbol}</h3>
                <span>
                  {t("holdings.meta", { shares, cost: formatMoney(holding.average_cost, currency) })}
                  {lastPrice !== null ? t("holdings.metaPrice", { price: formatMoney(holding.last_price!, currency) }) : t("holdings.noPrice")}
                  {holding.updated_at ? t("holdings.updatedAt", { date: formatDate(holding.updated_at) }) : ""}
                </span>
              </div>
              <strong>{formatMoney(shown.amount, shown.currency)}</strong>
              <div className="account-card-actions">
                {editing ? (
                  <>
                    <input className="inline-number" type="number" min="0" step="0.01" value={draft} onChange={(event) => setDraft(event.target.value)} placeholder={t("holdings.pricePlaceholder")} autoFocus />
                    <button className="row-action" onClick={() => { if (draft.trim()) onSetPrice(holding.id, draft.trim()); setEditingId(null); }} title={t("holdings.savePrice")} aria-label={t("holdings.savePrice")}><Check size={16} /></button>
                    <button className="row-action" onClick={() => setEditingId(null)} title={t("common.cancel")} aria-label={t("common.cancel")}><X size={16} /></button>
                  </>
                ) : (
                  <>
                    <button className="row-action" onClick={() => { setDraft(holding.last_price ?? ""); setEditingId(holding.id); }} title={t("holdings.updatePrice")} aria-label={t("holdings.updatePrice")}><RefreshCcw size={16} /></button>
                    {onRefreshHolding && (
                      <button className="text-button" onClick={() => void onRefreshHolding(holding.id)} title={t("holdings.forceRefresh")}>{t("holdings.refreshShort")}</button>
                    )}
                    <button className="text-button" onClick={() => onBuy(holding.symbol)}>{t("holdings.buyShort")}</button>
                    <button className="text-button" onClick={() => onSell(holding.symbol)}>{t("holdings.sellShort")}</button>
                  </>
                )}
              </div>
            </article>
          );
        })}
        {holdings.length === 0 && <EmptyState title={t("holdings.emptyTitle")} detail={t("holdings.emptyDetail")} />}
      </div>
    </section>
  );
}

export function EmptyState({ title, detail }: { title: string; detail: string }) {
  return <div className="empty-state"><span><ReceiptText size={20} /></span><div><strong>{title}</strong><p>{detail}</p></div></div>;
}
