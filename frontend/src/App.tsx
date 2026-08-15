import {
  ArrowDownLeft,
  ArrowLeftRight,
  ArrowUpRight,
  BadgeDollarSign,
  Banknote,
  BriefcaseBusiness,
  Building2,
  CalendarDays,
  Car,
  ChartNoAxesCombined,
  ChartCandlestick,
  Check,
  ChevronDown,
  CircleDollarSign,
  CircleEllipsis,
  Cloud,
  CreditCard,
  Dumbbell,
  Eye,
  EyeOff,
  Gamepad2,
  Gift,
  GraduationCap,
  Handshake,
  HeartPulse,
  House,
  Landmark,
  LayoutDashboard,
  Laptop,
  LoaderCircle,
  LockKeyhole,
  LogOut,
  Menu,
  Moon,
  MoreHorizontal,
  PawPrint,
  Percent,
  PiggyBank,
  Plane,
  Plus,
  ReceiptText,
  RefreshCcw,
  RotateCcw,
  Search,
  ShieldCheck,
  Shield,
  ShoppingBag,
  Smartphone,
  Sun,
  Tags,
  TrendingUp,
  Trophy,
  Trash2,
  Utensils,
  Users,
  WalletCards,
  X,
  Zap,
  type LucideIcon
} from "lucide-react";
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type CSSProperties,
  type FormEvent
} from "react";
import {
  ApiError,
  createAccount,
  createCategory,
  createDeposit,
  createLoan,
  createTransaction,
  createTransfer,
  deleteCategory,
  getAuthSession,
  loadAppData,
  login,
  logout,
  markReimbursable,
  reimburse,
  repayLoan,
  settleDeposit,
  voidTransaction
} from "./api";
import type {
  Account,
  AccountType,
  AppData,
  AuthSession,
  CashFlowSummary,
  Category,
  CategoryKind,
  Loan,
  LoanType,
  MonthlySummary,
  Transaction,
  TransactionKind
} from "./types";

type View = "dashboard" | "accounts" | "transactions" | "loans" | "insights";
type Modal = "transaction" | "account" | "category" | "deposit" | "settle" | "reimburse" | "loan" | "repay" | null;

const NAV_ITEMS: Array<{ id: View; label: string; icon: LucideIcon }> = [
  { id: "dashboard", label: "总览", icon: LayoutDashboard },
  { id: "accounts", label: "账户", icon: WalletCards },
  { id: "transactions", label: "交易", icon: ReceiptText },
  { id: "loans", label: "借贷", icon: Handshake },
  { id: "insights", label: "分析", icon: ChartNoAxesCombined }
];

const CATEGORY_COLORS = ["#274e3f", "#dd8d5b", "#7e95c9", "#d2ad58", "#8f6faf", "#669b92"];
const COMMON_CURRENCIES = ["CNY", "USD", "HKD", "EUR", "JPY", "GBP"];

const CATEGORY_VISUALS: Record<string, { icon: LucideIcon; color: string }> = {
  工资: { icon: BriefcaseBusiness, color: "#2c8765" },
  奖金: { icon: Trophy, color: "#c08a2f" },
  副业: { icon: Laptop, color: "#5078a5" },
  投资收益: { icon: ChartCandlestick, color: "#338c78" },
  利息: { icon: Percent, color: "#7a8f45" },
  报销: { icon: ReceiptText, color: "#6f76a8" },
  礼金: { icon: Gift, color: "#b26783" },
  退款: { icon: RotateCcw, color: "#4c918b" },
  其他收入: { icon: BadgeDollarSign, color: "#668562" },
  餐饮: { icon: Utensils, color: "#d0784e" },
  交通: { icon: Car, color: "#5077a5" },
  购物: { icon: ShoppingBag, color: "#ad6687" },
  居家: { icon: House, color: "#a47748" },
  娱乐: { icon: Gamepad2, color: "#7766a9" },
  医疗保健: { icon: HeartPulse, color: "#c55f64" },
  教育: { icon: GraduationCap, color: "#527ea0" },
  旅行: { icon: Plane, color: "#438b92" },
  通讯: { icon: Smartphone, color: "#6576a5" },
  水电燃气: { icon: Zap, color: "#bd8c30" },
  住房: { icon: Building2, color: "#8b704f" },
  保险: { icon: Shield, color: "#527d70" },
  数字订阅: { icon: Cloud, color: "#657fa8" },
  运动健身: { icon: Dumbbell, color: "#4f8e67" },
  宠物: { icon: PawPrint, color: "#a77457" },
  人情往来: { icon: Handshake, color: "#9b6c83" },
  家庭: { icon: Users, color: "#a06d50" },
  税费: { icon: Landmark, color: "#7e7165" },
  其他支出: { icon: CircleEllipsis, color: "#777b75" }
};

function categoryVisual(name: string) {
  const preset = CATEGORY_VISUALS[name];
  if (preset) return preset;
  const hash = [...name].reduce((value, character) => value + (character.codePointAt(0) ?? 0), 0);
  return { icon: Tags, color: CATEGORY_COLORS[hash % CATEGORY_COLORS.length] };
}

function CategoryAvatar({ name, size = "medium", className = "" }: { name: string; size?: "tiny" | "small" | "medium"; className?: string }) {
  const visual = categoryVisual(name);
  const Icon = visual.icon;
  const iconSize = size === "tiny" ? 11 : size === "small" ? 14 : 18;
  return (
    <span
      className={`category-avatar ${size} ${className}`}
      style={{ "--category-color": visual.color } as CSSProperties}
      aria-hidden="true"
    >
      <Icon size={iconSize} strokeWidth={1.9} />
    </span>
  );
}

function formatMoney(value: string, currency: string, compact = false): string {
  const number = Number(value);
  if (!Number.isFinite(number)) return `${value} ${currency}`;
  return new Intl.NumberFormat("zh-CN", {
    style: "currency",
    currency,
    currencyDisplay: "narrowSymbol",
    minimumFractionDigits: compact ? 0 : 2,
    maximumFractionDigits: compact ? 1 : 2,
    notation: compact ? "compact" : "standard"
  }).format(number);
}

function formatDate(value: string): string {
  return new Intl.DateTimeFormat("zh-CN", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit"
  }).format(new Date(value));
}

function currentMonthValue(): string {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}`;
}

function localDateTimeValue(): string {
  const now = new Date();
  const offset = now.getTimezoneOffset() * 60_000;
  return new Date(now.getTime() - offset).toISOString().slice(0, 16);
}

function accountIcon(account: Account): LucideIcon {
  if (account.account_type === "savings") return PiggyBank;
  if (account.account_type === "stock") return TrendingUp;
  if (account.account_type === "credit") return CreditCard;
  if (account.name.includes("现金")) return Banknote;
  return WalletCards;
}

function availableCurrencies(accounts: Account[], transactions: Transaction[] = []): string[] {
  return [...new Set([
    ...COMMON_CURRENCIES,
    ...accounts.map((account) => account.currency),
    ...transactions.map((transaction) => transaction.currency)
  ])];
}

function App() {
  const [session, setSession] = useState<AuthSession | null>(null);
  const [checkingSession, setCheckingSession] = useState(true);

  useEffect(() => {
    void getAuthSession()
      .then(setSession)
      .catch((reason) => {
        if (!(reason instanceof ApiError && reason.status === 401)) {
          console.error("Unable to check login session", reason);
        }
        setSession(null);
      })
      .finally(() => setCheckingSession(false));
  }, []);

  useEffect(() => {
    const handleUnauthorized = () => setSession(null);
    window.addEventListener("koku:unauthorized", handleUnauthorized);
    return () => window.removeEventListener("koku:unauthorized", handleUnauthorized);
  }, []);

  if (checkingSession) return <AuthLoadingState />;
  if (!session) return <LoginPage onAuthenticated={setSession} />;
  return (
    <LedgerApp
      username={session.username}
      onLogout={async () => {
        try { await logout(); }
        finally { setSession(null); }
      }}
    />
  );
}

function LoginPage({ onAuthenticated }: { onAuthenticated: (session: AuthSession) => void }) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dark, setDark] = useState(() => localStorage.getItem("koku-theme") === "dark");

  useEffect(() => {
    document.documentElement.dataset.theme = dark ? "dark" : "light";
    localStorage.setItem("koku-theme", dark ? "dark" : "light");
  }, [dark]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try { onAuthenticated(await login(username, password)); }
    catch (reason) {
      setError(reason instanceof ApiError && reason.status === 401 ? "用户名或密码不正确" : reason instanceof Error ? reason.message : "暂时无法登录");
      setSubmitting(false);
    }
  };

  return (
    <main className="login-page">
      <button className="login-theme-button" type="button" onClick={() => setDark((value) => !value)} aria-label="切换主题">
        {dark ? <Sun size={18} /> : <Moon size={18} />}
      </button>
      <section className="login-story" aria-label="Koku 私人账本">
        <div className="login-brand"><div className="brand-mark" aria-hidden="true"><span /><span /></div><div><strong>Koku</strong><small>PRIVATE LEDGER</small></div></div>
        <div className="login-story-copy">
          <span>YOUR MONEY, QUIETLY KEPT</span>
          <h1>只属于你的，<br />私人账本。</h1>
          <p>账户、交易和统计仅保存在你自己的服务器中。没有广告，没有第三方分析，也不会上传到其他平台。</p>
        </div>
        <div className="login-trust-row"><span><ShieldCheck size={16} />私有部署</span><span><LockKeyhole size={16} />加密会话</span></div>
      </section>
      <section className="login-panel">
        <form className="login-card" onSubmit={submit}>
          <div className="login-lock"><LockKeyhole size={20} /></div>
          <span className="login-eyebrow">WELCOME BACK</span>
          <h2>登录你的账本</h2>
          <p>验证身份后才能读取服务器中的财务数据。</p>
          <label><span>用户名</span><input autoFocus required autoComplete="username" value={username} onChange={(event) => setUsername(event.target.value)} placeholder="输入用户名" /></label>
          <label><span>密码</span><div className="password-field"><input required type={showPassword ? "text" : "password"} autoComplete="current-password" value={password} onChange={(event) => setPassword(event.target.value)} placeholder="输入密码" /><button type="button" onClick={() => setShowPassword((value) => !value)} aria-label={showPassword ? "隐藏密码" : "显示密码"}>{showPassword ? <EyeOff size={17} /> : <Eye size={17} />}</button></div></label>
          {error && <div className="login-error" role="alert">{error}</div>}
          <button className="login-submit" disabled={submitting || !username || !password}>{submitting ? <LoaderCircle className="spin" size={18} /> : <LockKeyhole size={17} />}{submitting ? "正在验证" : "安全登录"}</button>
          <small className="login-footnote">登录会话仅存储在此浏览器的安全 Cookie 中</small>
        </form>
      </section>
    </main>
  );
}

function LedgerApp({ username, onLogout }: { username: string; onLogout: () => Promise<void> }) {
  const [activeView, setActiveView] = useState<View>("dashboard");
  const [monthValue, setMonthValue] = useState(currentMonthValue);
  const [currency, setCurrency] = useState("CNY");
  const [data, setData] = useState<AppData | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [modal, setModal] = useState<Modal>(null);
  const [depositFrom, setDepositFrom] = useState<Account | null>(null);
  const [settleTarget, setSettleTarget] = useState<Account | null>(null);
  const [reimburseTarget, setReimburseTarget] = useState<Transaction | null>(null);
  const [loanTarget, setLoanTarget] = useState<Loan | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  const [dark, setDark] = useState(() => localStorage.getItem("koku-theme") === "dark");

  const [year, month] = monthValue.split("-").map(Number);

  const refresh = useCallback(
    async (quiet = false) => {
      if (quiet) setRefreshing(true);
      else setLoading(true);
      try {
        const nextData = await loadAppData(year, month, currency);
        setData(nextData);
        setError(null);
      } catch (reason) {
        setError(reason instanceof Error ? reason.message : "无法连接 Koku API");
      } finally {
        setLoading(false);
        setRefreshing(false);
      }
    },
    [currency, month, year]
  );

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    document.documentElement.dataset.theme = dark ? "dark" : "light";
    localStorage.setItem("koku-theme", dark ? "dark" : "light");
  }, [dark]);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 2600);
    return () => window.clearTimeout(timer);
  }, [toast]);

  const currencies = useMemo(() => {
    const values = new Set(data ? availableCurrencies(data.accounts, data.transactions) : COMMON_CURRENCIES);
    values.add(currency);
    return [...values];
  }, [currency, data]);

  const mutate = async (action: () => Promise<unknown>, successMessage: string) => {
    await action();
    setModal(null);
    setToast(successMessage);
    await refresh(true);
  };

  const content = (() => {
    if (loading && !data) return <LoadingState />;
    if (error && !data) return <ErrorState message={error} onRetry={() => void refresh()} />;
    if (!data) return null;
    switch (activeView) {
      case "accounts":
        return (
          <AccountsPage
            data={data}
            onAddAccount={() => setModal("account")}
            onDeposit={(account) => {
              setDepositFrom(account);
              setModal("deposit");
            }}
            onSettle={(account) => {
              setSettleTarget(account);
              setModal("settle");
            }}
          />
        );
      case "transactions":
        return (
          <TransactionsPage
            data={data}
            onAdd={() => setModal("transaction")}
            onVoid={(transaction) =>
              void mutate(() => voidTransaction(transaction.id), "交易已撤销，余额已恢复")
            }
            onMarkReimbursable={(transaction) =>
              void mutate(() => markReimbursable(transaction.id), "已标记为待报销")
            }
            onReimburse={(transaction) => {
              setReimburseTarget(transaction);
              setModal("reimburse");
            }}
          />
        );
      case "loans":
        return (
          <LoansPage
            loans={data.loans}
            accounts={data.accounts}
            onCreateLoan={() => setModal("loan")}
            onRepay={(loan) => {
              setLoanTarget(loan);
              setModal("repay");
            }}
          />
        );
      case "insights":
        return <InsightsPage summary={data.monthly} cashFlow={data.cashFlow} />;
      default:
        return (
          <Dashboard
            data={data}
            onAdd={() => setModal("transaction")}
            onShowTransactions={() => setActiveView("transactions")}
          />
        );
    }
  })();

  return (
    <div className="app-shell">
      <aside className={`sidebar ${mobileNavOpen ? "sidebar-open" : ""}`}>
        <div className="brand">
          <div className="brand-mark" aria-hidden="true">
            <span />
            <span />
          </div>
          <div>
            <strong>Koku</strong>
            <small>PRIVATE LEDGER</small>
          </div>
          <button className="mobile-close" onClick={() => setMobileNavOpen(false)} aria-label="关闭菜单">
            <X size={20} />
          </button>
        </div>

        <nav className="primary-nav" aria-label="主导航">
          {NAV_ITEMS.map(({ id, label, icon: Icon }) => (
            <button
              className={activeView === id ? "active" : ""}
              key={id}
              onClick={() => {
                setActiveView(id);
                setMobileNavOpen(false);
              }}
            >
              <Icon size={19} strokeWidth={1.8} />
              <span>{label}</span>
            </button>
          ))}
        </nav>

        <div className="sidebar-spacer" />
        <div className="privacy-note">
          <ShieldCheck size={18} />
          <div>
            <strong>私有部署</strong>
            <span>数据保存在你的服务器</span>
          </div>
        </div>
        <div className="profile-actions">
          <button className="profile-chip" onClick={() => setModal("category")}>
            <span className="avatar">K</span>
            <span>
              <strong>{username}</strong>
              <small>管理分类</small>
            </span>
            <MoreHorizontal size={18} />
          </button>
          <button className="logout-button" onClick={() => void onLogout()} aria-label="退出登录" title="退出登录"><LogOut size={17} /></button>
        </div>
      </aside>

      {mobileNavOpen && <button className="nav-scrim" onClick={() => setMobileNavOpen(false)} />}

      <main className="main-content">
        <header className="topbar">
          <button className="icon-button mobile-menu" onClick={() => setMobileNavOpen(true)} aria-label="打开菜单">
            <Menu size={20} />
          </button>
          <div className="period-control">
            <CalendarDays size={17} />
            <input
              aria-label="统计月份"
              type="month"
              value={monthValue}
              onChange={(event) => setMonthValue(event.target.value)}
            />
          </div>
          <div className="topbar-actions">
            <label className="currency-select">
              <select aria-label="显示币种" value={currency} onChange={(event) => setCurrency(event.target.value)}>
                {currencies.map((item) => (
                  <option key={item}>{item}</option>
                ))}
              </select>
              <ChevronDown size={14} />
            </label>
            <button
              className={`icon-button ${refreshing ? "spinning" : ""}`}
              onClick={() => void refresh(true)}
              aria-label="刷新数据"
            >
              <RefreshCcw size={18} />
            </button>
            <button className="icon-button" onClick={() => setDark((value) => !value)} aria-label="切换主题">
              {dark ? <Sun size={18} /> : <Moon size={18} />}
            </button>
            <button className="primary-button compact" onClick={() => setModal("transaction")}>
              <Plus size={18} />
              <span>记一笔</span>
            </button>
          </div>
        </header>

        {error && data && <div className="inline-error">同步失败：{error}</div>}
        {content}
      </main>

      <nav className="mobile-bottom-nav" aria-label="移动端导航">
        {NAV_ITEMS.map(({ id, label, icon: Icon }) => (
          <button key={id} className={activeView === id ? "active" : ""} onClick={() => setActiveView(id)}>
            <Icon size={20} />
            <span>{label}</span>
          </button>
        ))}
        <button className="mobile-add" onClick={() => setModal("transaction")} aria-label="记一笔">
          <Plus size={23} />
        </button>
      </nav>

      {modal === "transaction" && data && (
        <TransactionModal
          accounts={data.accounts}
          categories={data.categories}
          onClose={() => setModal(null)}
          onSubmit={(input) =>
            mutate(
              () =>
                input.kind === "transfer"
                  ? createTransfer(input.payload)
                  : createTransaction(input.payload),
              input.kind === "transfer" ? "转账完成" : "交易已记录"
            )
          }
        />
      )}
      {modal === "account" && (
        <AccountModal
          onClose={() => setModal(null)}
          onSubmit={(input) => mutate(() => createAccount(input), "账户已创建")}
        />
      )}
      {modal === "category" && (
        <CategoryModal
          categories={data?.categories ?? []}
          onClose={() => setModal(null)}
          onSubmit={(input) => mutate(() => createCategory(input), "分类已创建")}
          onDelete={async (category) => {
            await deleteCategory(category.id);
            setToast(`“${category.name}”已删除，历史账单保持不变`);
            await refresh(true);
          }}
        />
      )}
      {modal === "deposit" && data && depositFrom && (
        <DepositModal
          source={depositFrom}
          onClose={() => setModal(null)}
          onSubmit={(input) =>
            mutate(
              () => createDeposit({ from_account_id: depositFrom.id, ...input }),
              "已转为定期存款"
            )
          }
        />
      )}
      {modal === "settle" && data && settleTarget && (
        <SettleDepositModal
          deposit={settleTarget}
          accounts={data.accounts}
          onClose={() => setModal(null)}
          onSubmit={(to_account_id) =>
            mutate(
              () => settleDeposit(settleTarget.id, to_account_id),
              "定期已结清，本息已转回"
            )
          }
        />
      )}
      {modal === "reimburse" && data && reimburseTarget && (
        <ReimburseModal
          expense={reimburseTarget}
          accounts={data.accounts}
          onClose={() => setModal(null)}
          onSubmit={(input) =>
            mutate(
              () => reimburse({ expense_id: reimburseTarget.id, ...input }),
              "报销完成"
            )
          }
        />
      )}
      {modal === "loan" && data && (
        <LoanModal
          accounts={data.accounts}
          onClose={() => setModal(null)}
          onSubmit={(input) => mutate(() => createLoan(input), "借款已记录")}
        />
      )}
      {modal === "repay" && data && loanTarget && (
        <RepayModal
          loan={loanTarget}
          accounts={data.accounts}
          onClose={() => setModal(null)}
          onSubmit={(input) =>
            mutate(
              () => repayLoan(loanTarget.id, input),
              "还款已入账"
            )
          }
        />
      )}
      {toast && (
        <div className="toast" role="status">
          <Check size={17} /> {toast}
        </div>
      )}
    </div>
  );
}

function PageTitle({ eyebrow, title, actions }: { eyebrow: string; title: string; actions?: React.ReactNode }) {
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

function Dashboard({
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

function healthScore(summary: MonthlySummary): number {
  const income = Number(summary.total_income);
  const expense = Number(summary.total_expense);
  if (income <= 0) return expense === 0 ? 100 : 0;
  return Math.max(0, Math.min(100, Math.round(((income - expense) / income) * 100)));
}

function TrendChart({ transactions, currency }: { transactions: Transaction[]; currency: string }) {
  const points = useMemo(() => {
    const values = Array.from({ length: 12 }, (_, index) => ({ x: index, value: 0 }));
    for (const item of transactions) {
      if (item.kind === "transfer" || item.currency !== currency) continue;
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

function AccountMiniCard({ account, hidden, index }: { account: Account; hidden: boolean; index: number }) {
  const Icon = accountIcon(account);
  return (
    <article className={`account-mini tone-${index % 4}`}>
      <div><span className="account-icon"><Icon size={19} /></span><MoreHorizontal size={18} /></div>
      <small>{account.account_type === "credit" ? "信用（负债）" : ({ cash: "零钱账户", savings: "储蓄账户", stock: "股票账户" } as Record<AccountType, string>)[account.account_type]}</small>
      <h3>{account.name}</h3>
      <strong>{hidden ? "••••••" : formatMoney(account.balance, account.currency)}</strong>
      <span className="currency-badge">{account.currency}</span>
    </article>
  );
}

function AccountsPage({
  data,
  onAddAccount,
  onDeposit,
  onSettle
}: {
  data: AppData;
  onAddAccount: () => void;
  onDeposit: (account: Account) => void;
  onSettle: (account: Account) => void;
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
      <AccountGroup title="零钱" subtitle={`${cash.length} 个账户`} accounts={cash} />
      <AccountGroup
        title="储蓄"
        subtitle={`${savings.length} 个账户`}
        accounts={savings}
        renderAction={(account) => (
          account.interest_rate
            ? <button className="row-action" title="结清定期并转回" aria-label="结清定期" onClick={() => onSettle(account)}><RotateCcw size={16} /></button>
            : <button className="row-action" title="转入定期" aria-label="转入定期" onClick={() => onDeposit(account)}><PiggyBank size={16} /></button>
        )}
      />
      <AccountGroup title="股票" subtitle={`${stock.length} 个账户`} accounts={stock} />
      <AccountGroup title="信用" subtitle={`${credit.length} 个账户（负债）`} accounts={credit} />
    </div>
  );
}

function SummaryCard({ label, value, currency, tone }: { label: string; value: string; currency: string; tone: string }) {
  return (
    <article className={`summary-card ${tone}`}>
      <span>{label}</span>
      <strong>{formatMoney(value, currency)}</strong>
      <small>以 {currency} 计价</small>
    </article>
  );
}

function AccountGroup({
  title,
  subtitle,
  accounts,
  renderAction
}: {
  title: string;
  subtitle: string;
  accounts: Account[];
  renderAction?: (account: Account) => React.ReactNode;
}) {
  return (
    <section className="section-block account-group">
      <div className="section-heading compact-heading"><div><span>{subtitle}</span><h2>{title}</h2></div></div>
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
                  {account.interest_rate && account.maturity_at
                    ? ` · 定期 ${account.interest_rate}% · ${formatDate(account.maturity_at)}到期`
                    : ""}
                </span>
              </div>
              <strong>{formatMoney(account.balance, account.currency)}</strong>
              {renderAction
                ? renderAction(account)
                : <button className="bare-button" aria-label={`${account.name}更多操作`}><MoreHorizontal size={19} /></button>}
            </article>
          );
        })}
        {accounts.length === 0 && <EmptyState title="这里还没有账户" detail="新建账户后即可开始记账。" />}
      </div>
    </section>
  );
}

function TransactionsPage({
  data,
  onAdd,
  onVoid,
  onMarkReimbursable,
  onReimburse
}: {
  data: AppData;
  onAdd: () => void;
  onVoid: (transaction: Transaction) => void;
  onMarkReimbursable: (transaction: Transaction) => void;
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
            onReimburse={() => onReimburse(transaction)}
          />
        ))}
        {filtered.length === 0 && <EmptyState title="没有找到交易" detail="换个关键词，或记录一笔新的交易。" />}
      </article>
    </div>
  );
}

function TransactionList({ transactions, accounts, categories }: { transactions: Transaction[]; accounts: Account[]; categories: Category[] }) {
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

function TransactionRow({
  transaction,
  account,
  target,
  category,
  compact = false,
  onVoid,
  onMarkReimbursable,
  onReimburse
}: {
  transaction: Transaction;
  account?: Account;
  target?: Account;
  category?: Category;
  compact?: boolean;
  onVoid?: () => void;
  onMarkReimbursable?: () => void;
  onReimburse?: () => void;
}) {
  const meta = {
    expense: { icon: ArrowUpRight, label: category?.name ?? "支出", className: "expense" },
    income: { icon: ArrowDownLeft, label: category?.name ?? "收入", className: "income" },
    transfer: { icon: ArrowLeftRight, label: "账户转账", className: "transfer" },
    loan: { icon: Handshake, label: transaction.note || "借款", className: "transfer" }
  }[transaction.kind];
  const Icon = meta.icon;
  const prefix = transaction.kind === "expense" ? "−" : transaction.kind === "income" ? "+" : "";
  const reimbursable = transaction.reimbursable_at && !transaction.reimbursed_at;
  return (
    <div className={`transaction-row ${compact ? "compact-row" : ""} ${transaction.voided_at ? "voided" : ""}`}>
      <div className="transaction-main">
        {transaction.kind === "transfer" || transaction.kind === "loan" ? (
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
        {transaction.kind !== "transfer" && transaction.kind !== "loan" && account && transaction.currency !== account.currency && (
          <span>入账 {formatMoney(transaction.settled_amount, account.currency)}</span>
        )}
        {transaction.kind === "expense" && transaction.reimbursed_amount !== "0" && (
          <span>已报销 {formatMoney(transaction.reimbursed_amount, transaction.currency)}</span>
        )}
        {compact && <span>{formatDate(transaction.occurred_at)}</span>}
      </div>
      {!compact && (
        <div className="row-actions">
          {transaction.kind === "expense" && !transaction.voided_at && (
            reimbursable
              ? <button className="row-action reimburse" onClick={onReimburse} title="报销" aria-label="报销"><BadgeDollarSign size={16} /></button>
              : !transaction.reimbursed_at && (
                  <button className="row-action reimburse" onClick={onMarkReimbursable} title="标记待报销" aria-label="标记待报销"><Tags size={16} /></button>
                )
          )}
          <button
            className="row-action"
            disabled={Boolean(transaction.voided_at) || transaction.kind === "loan"}
            onClick={onVoid}
            title="撤销并恢复余额"
            aria-label="撤销交易"
          ><Trash2 size={16} /></button>
        </div>
      )}
    </div>
  );
}

function InsightsPage({ summary, cashFlow }: { summary: MonthlySummary; cashFlow: CashFlowSummary }) {
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
        <div><span>KOKU NOTE</span><h3>你保留了 {healthScore(summary)}% 的本月收入</h3><p>结余率来自已确认的收入与支出，不包含账户间转账。</p></div>
      </article>
    </div>
  );
}

interface SankeyDatum {
  id: string;
  name: string;
  amount: number;
  amountText: string;
  color: string;
}

interface SankeyNode extends SankeyDatum {
  y: number;
  height: number;
}

function MobileCashFlowGroup({
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

function CashFlowSankey({ summary }: { summary: CashFlowSummary }) {
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
        <small>收入来源 → 可用现金 → 支出去向与结余</small>
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

function sankeyRibbonPath(
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

function buildDonutGradient(summary: MonthlySummary): string {
  if (!summary.expenses_by_category.length) return "var(--border) 0 100%";
  let cursor = 0;
  return summary.expenses_by_category.map((item) => {
    const start = cursor;
    cursor += Number(item.percentage);
    return `${categoryVisual(item.category_name).color} ${start}% ${cursor}%`;
  }).join(", ");
}

function CategoryBars({ summary, detailed = false }: { summary: MonthlySummary; detailed?: boolean }) {
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

function LoansPage({
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
    <div className="page page-enter">
      <PageTitle
        eyebrow="LOANS"
        title="借入与借出"
        actions={<button className="primary-button" onClick={onCreateLoan}><Plus size={18} /> 记一笔借款</button>}
      />
      <section className="balance-summary-row">
        <SummaryCard label="借出应收" value={lendOutstanding.toFixed(2)} currency={currency} tone="green" />
        <SummaryCard label="借入应付" value={borrowOutstanding.toFixed(2)} currency={currency} tone="orange" />
      </section>
      <section className="section-block account-group">
        <div className="section-heading compact-heading"><div><span>{open.length} 笔未结清</span><h2>进行中</h2></div></div>
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
      </section>
      {closed.length > 0 && (
        <section className="section-block account-group">
          <div className="section-heading compact-heading"><div><span>{closed.length} 笔已结清</span><h2>已结清</h2></div></div>
          <div className="account-grid">
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
        </section>
      )}
    </div>
  );
}

function DepositModal({
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

function SettleDepositModal({
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

function ReimburseModal({
  expense,
  accounts,
  onClose,
  onSubmit
}: {
  expense: Transaction;
  accounts: Account[];
  onClose: () => void;
  onSubmit: (input: { account_id: number; amount: string; note?: string }) => Promise<void>;
}) {
  const remaining = Math.max(0, Number(expense.amount) - Number(expense.reimbursed_amount)).toFixed(2);
  const [accountId, setAccountId] = useState("");
  const [amount, setAmount] = useState(remaining);
  const [note, setNote] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      await onSubmit({ account_id: Number(accountId), amount, note: note || undefined });
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
            <select required value={accountId} onChange={(e) => setAccountId(e.target.value)}>
              <option value="" disabled>选择账户</option>
              {accounts.map((account) => (
                <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>
              ))}
            </select>
          </label>
          <label><span>报销金额</span><input required step="0.01" inputMode="decimal" max={remaining} value={amount} onChange={(e) => setAmount(e.target.value)} /></label>
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

function LoanModal({
  accounts,
  onClose,
  onSubmit
}: {
  accounts: Account[];
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
          <label><span>往来人</span><input required autoFocus value={counterparty} onChange={(e) => setCounterparty(e.target.value)} placeholder="例如：张三" /></label>
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

function RepayModal({
  loan,
  accounts,
  onClose,
  onSubmit
}: {
  loan: Loan;
  accounts: Account[];
  onClose: () => void;
  onSubmit: (input: { account_id: number; amount: string; note?: string }) => Promise<void>;
}) {
  const [accountId, setAccountId] = useState("");
  const [amount, setAmount] = useState(loan.outstanding);
  const [note, setNote] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      await onSubmit({ account_id: Number(accountId), amount, note: note || undefined });
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
          <label><span>还款金额</span><input required step="0.01" inputMode="decimal" max={loan.outstanding} value={amount} onChange={(e) => setAmount(e.target.value)} /></label>
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

function ModalShell({ title, eyebrow, onClose, children }: { title: string; eyebrow: string; onClose: () => void; children: React.ReactNode }) {
  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section className="modal-card" role="dialog" aria-modal="true" aria-label={title}>
        <header><div><span>{eyebrow}</span><h2>{title}</h2></div><button className="icon-button" onClick={onClose}><X size={19} /></button></header>
        {children}
      </section>
    </div>
  );
}

type TransactionSubmit =
  | { kind: "expense" | "income"; payload: Parameters<typeof createTransaction>[0] }
  | { kind: "transfer"; payload: Parameters<typeof createTransfer>[0] };

function TransactionModal({
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
  const [kind, setKind] = useState<Exclude<TransactionKind, "loan">>("expense");
  const [accountId, setAccountId] = useState(accounts[0]?.id ?? 0);
  const [targetId, setTargetId] = useState(accounts[1]?.id ?? accounts[0]?.id ?? 0);
  const [sourceCurrency, setSourceCurrency] = useState(accounts[0]?.currency ?? "CNY");
  const [categoryId, setCategoryId] = useState(categories.find((item) => item.kind === "expense")?.id ?? 0);
  const [amount, setAmount] = useState("");
  const [settledAmount, setSettledAmount] = useState("");
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

  const changeKind = (nextKind: Exclude<TransactionKind, "loan">) => {
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
          {foreignTransaction && <label><span>计入账户余额 · {source?.currency}</span><input required min="0.01" step="0.01" inputMode="decimal" value={settledAmount} onChange={(e) => setSettledAmount(e.target.value)} placeholder="换算后的结算金额" /></label>}
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

function AccountModal({ onClose, onSubmit }: { onClose: () => void; onSubmit: (input: Parameters<typeof createAccount>[0]) => Promise<void> }) {
  const [name, setName] = useState("");
  const [type, setType] = useState<AccountType>("cash");
  const [currency, setCurrency] = useState("CNY");
  const [balance, setBalance] = useState("0");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = async (event: FormEvent) => {
    event.preventDefault(); setSubmitting(true); setError(null);
    try { await onSubmit({ name, account_type: type, currency, opening_balance: balance }); }
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
            <option value="credit">信用（负债）</option>
          </select></label>
          <label><span>账户结算币种</span><input required maxLength={3} value={currency} onChange={(e) => setCurrency(e.target.value.toUpperCase())} /></label>
          <label className="span-two"><span>期初余额</span><input required step="0.01" inputMode="decimal" value={balance} onChange={(e) => setBalance(e.target.value)} /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions"><button type="button" className="secondary-button" onClick={onClose}>取消</button><button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}创建账户</button></div>
      </form>
    </ModalShell>
  );
}

function CategoryModal({ categories, onClose, onSubmit, onDelete }: { categories: Category[]; onClose: () => void; onSubmit: (input: Parameters<typeof createCategory>[0]) => Promise<void>; onDelete: (category: Category) => Promise<void> }) {
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
        <p className="category-delete-note">删除后仅从新交易的可选分类中隐藏，历史账单和统计会完整保留。</p>
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

function AuthLoadingState() {
  return <main className="auth-loading"><div className="loading-mark"><span /><span /></div><p>正在验证安全会话…</p></main>;
}

function LoadingState() {
  return <div className="loading-state"><div className="loading-mark"><span /><span /></div><p>正在打开你的账本…</p></div>;
}

function ErrorState({ message, onRetry }: { message: string; onRetry: () => void }) {
  return <div className="error-state"><CircleDollarSign size={34} /><h2>暂时无法读取账本</h2><p>{message}</p><button className="primary-button" onClick={onRetry}><RefreshCcw size={17} />重新连接</button></div>;
}

function EmptyState({ title, detail }: { title: string; detail: string }) {
  return <div className="empty-state"><span><ReceiptText size={20} /></span><div><strong>{title}</strong><p>{detail}</p></div></div>;
}

export default App;
