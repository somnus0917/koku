import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type FormEvent
} from "react";
import {
  Bell,
  CalendarDays,
  ChartNoAxesCombined,
  Check,
  ChevronDown,
  CircleDollarSign,
  Eye,
  EyeOff,
  Globe,
  KeyRound,
  LayoutDashboard,
  LoaderCircle,
  LockKeyhole,
  LogOut,
  Mail,
  Menu,
  Monitor,
  Moon,
  MoreHorizontal,
  Plus,
  ReceiptText,
  RefreshCcw,
  Settings,
  ShieldCheck,
  Sun,
  Users,
  WalletCards,
  X,
  type LucideIcon
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { changeLanguage, uiLocale } from "./i18n";
import {
  ApiError,
  adjustBalance,
  buyStock,
  changePassword,
  clearBudget,
  createAccount,
  createCategory,
  createDeposit,
  createLoan,
  createRecurringRule,
  createTransaction,
  createTransfer,
  deleteCategory,
  deleteRecurringRule,
  getAuthSession,
  loadReminders,
  loadSummaryData,
  loadTransactions,
  login,
  logout,
  markReimbursable,
  refreshHoldings,
  refreshHolding,
  reimburse,
  repayLoan,
  rolloverBudgets,
  runRecurring,
  sellStock,
  sendReminderDigest,
  setBudget,
  setHoldingPrice,
  settleDeposit,
  unmarkReimbursable,
  updateAccount,
  updateTransaction,
  uploadReceipt,
  verifyTotp,
  voidTransaction,
  restoreTransaction,
  deleteTransactionPermanently
} from "./api";
import {
  AccountModal,
  CategoryModal,
  DepositModal,
  EditAccountModal,
  EditTransactionModal,
  ImportModal,
  LoanModal,
  PasswordModal,
  ReconciliationModal,
  RecurringModal,
  ReimburseModal,
  RepayModal,
  SettleDepositModal,
  TotpModal,
  TradeModal,
  TransactionModal
} from "./components/modals";
import {
  AccountsPage,
  Dashboard,
  InsightsPage,
  LoansSection,
  TransactionsPage
} from "./components/ledger";
import { UsersAdminPage } from "./components/users";
import { SystemAdminPage } from "./components/system";
import { useTheme } from "./theme";
import {
  COMMON_CURRENCIES,
  availableCurrencies,
  currentMonthValue,
  formatMoney,
  localDateTimeValue
} from "./lib";
import type {
  Account,
  AccountType,
  AppData,
  AuthSession,
  CashFlowSummary,
  Category,
  CategoryKind,
  Deposit,
  Loan,
  LoanType,
  MonthlySummary,
  ReminderItem,
  Transaction,
  TransactionKind,
  UserRole
} from "./types";

type View = "dashboard" | "accounts" | "transactions" | "insights" | "users" | "system";
type Modal = "transaction" | "account" | "category" | "deposit" | "settle" | "reimburse" | "loan" | "repay" | "edit-account" | "edit-transaction" | "recurring" | "trade" | "password" | "import" | "totp" | "reconcile" | null;

const NAV_ITEMS: Array<{ id: View; icon: LucideIcon }> = [
  { id: "dashboard", icon: LayoutDashboard },
  { id: "accounts", icon: WalletCards },
  { id: "transactions", icon: ReceiptText },
  { id: "insights", icon: ChartNoAxesCombined },
  { id: "users", icon: Users },
  { id: "system", icon: Settings }
];

/** 「全部月份」模式下的分页大小；单月模式一次性取上限（单月很少超过）。 */
const TRANSACTIONS_PAGE_SIZE = 200;
const MONTH_TRANSACTIONS_LIMIT = 1000;

/** 提醒到期日展示：YYYY-MM-DD / RFC3339 → "8月20日"（随界面语言变化）。 */
function formatReminderDay(value: string): string {
  const date = /^\d{4}-\d{2}-\d{2}$/.test(value) ? new Date(`${value}T00:00:00`) : new Date(value);
  return new Intl.DateTimeFormat(uiLocale(), { month: "long", day: "numeric" }).format(date);
}

export default function App() {
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
      role={session.role}
      userId={session.id}
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
  const [step, setStep] = useState<"credentials" | "totp">("credentials");
  const [totpToken, setTotpToken] = useState("");
  const [code, setCode] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { theme, setTheme } = useTheme();
  const { t, i18n } = useTranslation();

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      const result = await login(username, password);
      // 已启用二步验证：先拿 totp_token，切到动态码步骤再完成登录。
      if ("totp_required" in result) {
        setTotpToken(result.totp_token);
        setCode("");
        setStep("totp");
        setSubmitting(false);
        return;
      }
      onAuthenticated(result);
    } catch (reason) {
      setError(reason instanceof ApiError && reason.status === 401 ? t("login.invalidCredentials") : reason instanceof Error ? reason.message : t("login.unavailable"));
      setSubmitting(false);
    }
  };

  const submitTotp = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      onAuthenticated(await verifyTotp(totpToken, code.trim()));
    } catch (reason) {
      setError(reason instanceof ApiError && reason.status === 401 ? t("login.totpInvalid") : reason instanceof Error ? reason.message : t("login.unavailable"));
      setSubmitting(false);
    }
  };

  return (
    <main className="login-page">
      <div className="login-corner-actions">
        <button
          className="login-theme-button"
          type="button"
          onClick={() => setTheme(theme === "light" ? "dark" : theme === "dark" ? "system" : "light")}
          aria-label={t("common.themeToggle")}
          title={theme === "light" ? t("common.themeLight") : theme === "dark" ? t("common.themeDark") : t("common.themeSystem")}
        >
          {theme === "light" ? <Sun size={18} /> : theme === "dark" ? <Moon size={18} /> : <Monitor size={18} />}
        </button>
        <button
          className="login-theme-button"
          type="button"
          onClick={() => void changeLanguage(i18n.language?.toLowerCase().startsWith("en") ? "zh" : "en")}
          aria-label={t("common.language")}
          title={t("common.language")}
        >
          <Globe size={18} />
        </button>
      </div>
      <section className="login-story" aria-label={t("login.storyLabel")}>
        <div className="login-brand"><div className="brand-mark" aria-hidden="true"><span /><span /></div><div><strong>Koku</strong><small>PRIVATE LEDGER</small></div></div>
        <div className="login-story-copy">
          <span>YOUR MONEY, QUIETLY KEPT</span>
          <h1>{t("login.headlineLine1")}<br />{t("login.headlineLine2")}</h1>
          <p>{t("login.blurb")}</p>
        </div>
        <div className="login-trust-row"><span><ShieldCheck size={16} />{t("login.selfHosted")}</span><span><LockKeyhole size={16} />{t("login.encryptedSession")}</span></div>
      </section>
      <section className="login-panel">
        {step === "totp" ? (
          <form className="login-card" onSubmit={submitTotp}>
            <div className="login-lock"><ShieldCheck size={20} /></div>
            <span className="login-eyebrow">TWO-FACTOR AUTH</span>
            <h2>{t("totp.title")}</h2>
            <p>{t("login.totpIntro")}</p>
            <label><span>{t("login.totpCode")}</span><input autoFocus required inputMode="numeric" maxLength={6} pattern="[0-9]*" autoComplete="one-time-code" value={code} onChange={(event) => setCode(event.target.value)} placeholder={t("totp.codePlaceholder")} /></label>
            {error && <div className="login-error" role="alert">{error}</div>}
            <button className="login-submit" disabled={submitting || code.trim().length !== 6}>{submitting ? <LoaderCircle className="spin" size={18} /> : <ShieldCheck size={17} />}{submitting ? t("login.verifying") : t("login.verifyAndLogin")}</button>
            <button type="button" className="login-back" onClick={() => { setStep("credentials"); setError(null); setCode(""); }}>{t("login.backToCredentials")}</button>
          </form>
        ) : (
          <form className="login-card" onSubmit={submit}>
            <div className="login-lock"><LockKeyhole size={20} /></div>
            <span className="login-eyebrow">WELCOME BACK</span>
            <h2>{t("login.title")}</h2>
            <p>{t("login.subtitle")}</p>
            <label><span>{t("login.username")}</span><input autoFocus required autoComplete="username" value={username} onChange={(event) => setUsername(event.target.value)} placeholder={t("login.usernamePlaceholder")} /></label>
            <label><span>{t("login.password")}</span><div className="password-field"><input required type={showPassword ? "text" : "password"} autoComplete="current-password" value={password} onChange={(event) => setPassword(event.target.value)} placeholder={t("login.passwordPlaceholder")} /><button type="button" onClick={() => setShowPassword((value) => !value)} aria-label={showPassword ? t("login.hidePassword") : t("login.showPassword")}>{showPassword ? <EyeOff size={17} /> : <Eye size={17} />}</button></div></label>
            {error && <div className="login-error" role="alert">{error}</div>}
            <button className="login-submit" disabled={submitting || !username || !password}>{submitting ? <LoaderCircle className="spin" size={18} /> : <LockKeyhole size={17} />}{submitting ? t("login.verifying") : t("login.signIn")}</button>
            <small className="login-footnote">{t("login.footnote")}</small>
          </form>
        )}
      </section>
    </main>
  );
}

function LedgerApp({ username, role, userId, onLogout }: { username: string; role: UserRole; userId: number; onLogout: () => Promise<void> }) {
  const [activeView, setActiveView] = useState<View>("dashboard");
  const [monthValue, setMonthValue] = useState(currentMonthValue);
  const [currency, setCurrency] = useState("CNY");
  const [data, setData] = useState<AppData | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [modal, setModal] = useState<Modal>(null);
  const [editAccount, setEditAccount] = useState<Account | null>(null);
  const [editTransaction, setEditTransaction] = useState<Transaction | null>(null);
  const [depositFrom, setDepositFrom] = useState<Account | null>(null);
  const [settleTarget, setSettleTarget] = useState<Deposit | null>(null);
  const [reimburseTarget, setReimburseTarget] = useState<Transaction | null>(null);
  const [loanTarget, setLoanTarget] = useState<Loan | null>(null);
  const [reconcileAccount, setReconcileAccount] = useState<Account | null>(null);
  const [tradeSide, setTradeSide] = useState<"buy" | "sell">("buy");
  const [tradeSymbol, setTradeSymbol] = useState("");
  const [toast, setToast] = useState<string | null>(null);
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  const [reminders, setReminders] = useState<ReminderItem[]>([]);
  const [reminderOpen, setReminderOpen] = useState(false);
  const [reminderSending, setReminderSending] = useState(false);
  const [reminderError, setReminderError] = useState<string | null>(null);
  const reminderRef = useRef<HTMLDivElement | null>(null);
  const { theme, setTheme } = useTheme();
  const { t, i18n } = useTranslation();
  const [txOffset, setTxOffset] = useState(0);
  const [txHasMore, setTxHasMore] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);

  // monthValue 为空字符串表示「全部月份」；此时 year/month 为 undefined。
  const monthParts = monthValue ? monthValue.split("-").map(Number) : [];
  const year = monthParts[0];
  const month = monthParts[1];

  const refresh = useCallback(
    async (quiet = false) => {
      if (quiet) setRefreshing(true);
      else setLoading(true);
      try {
        // 先触发周期交易到期生成与月度预算自动延续（均请求驱动、无后台任务），再读取最新数据。
        await runRecurring().catch(() => undefined);
        await rolloverBudgets().catch(() => undefined);
        const now = new Date();
        const summaryYear = year ?? now.getFullYear();
        const summaryMonth = month ?? now.getMonth() + 1;
        const limit = month === undefined ? TRANSACTIONS_PAGE_SIZE : MONTH_TRANSACTIONS_LIMIT;
        const [summary, transactions] = await Promise.all([
          loadSummaryData(summaryYear, summaryMonth, currency),
          loadTransactions(0, limit, year, month)
        ]);
        setData({ ...summary, transactions });
        setTxOffset(transactions.length);
        setTxHasMore(month === undefined && transactions.length === TRANSACTIONS_PAGE_SIZE);
        setError(null);
      } catch (reason) {
        setError(reason instanceof Error ? reason.message : t("app.apiUnreachable"));
      } finally {
        setLoading(false);
        setRefreshing(false);
      }
    },
    [currency, month, year, t]
  );

  const loadMore = useCallback(async () => {
    if (loadingMore || !txHasMore) return;
    setLoadingMore(true);
    try {
      const page = await loadTransactions(txOffset, TRANSACTIONS_PAGE_SIZE);
      setData((current) =>
        current ? { ...current, transactions: [...current.transactions, ...page] } : current
      );
      setTxOffset((offset) => offset + page.length);
      setTxHasMore(page.length === TRANSACTIONS_PAGE_SIZE);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("app.loadMoreFailed"));
    } finally {
      setLoadingMore(false);
    }
  }, [loadingMore, txHasMore, txOffset, t]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 2600);
    return () => window.clearTimeout(timer);
  }, [toast]);

  // 提醒铃铛：数据每次刷新完成后重新拉取（未来 30 天到期项），失败静默。
  useEffect(() => {
    let cancelled = false;
    loadReminders(30)
      .then((items) => {
        if (!cancelled) setReminders(items);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [data]);

  // 点击提醒面板外部时关闭。
  useEffect(() => {
    if (!reminderOpen) return;
    const close = (event: MouseEvent) => {
      if (reminderRef.current && !reminderRef.current.contains(event.target as Node)) {
        setReminderOpen(false);
      }
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [reminderOpen]);

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

  const sendDigest = async () => {
    setReminderSending(true);
    setReminderError(null);
    try {
      const result = await sendReminderDigest();
      setToast(t("reminders.digestSent", { count: result.count }));
      setReminderOpen(false);
    } catch (reason) {
      setReminderError(reason instanceof Error ? reason.message : t("reminders.sendFailed"));
    } finally {
      setReminderSending(false);
    }
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
            onEdit={(account) => {
              setEditAccount(account);
              setModal("edit-account");
            }}
            onDeposit={(account) => {
              setDepositFrom(account);
              setModal("deposit");
            }}
            onSettle={(deposit) => {
              setSettleTarget(deposit);
              setModal("settle");
            }}
            onCreateLoan={() => setModal("loan")}
            onRepay={(loan) => {
              setLoanTarget(loan);
              setModal("repay");
            }}
            onCreateRecurring={() => setModal("recurring")}
            onDeleteRecurring={(id) =>
              void mutate(() => deleteRecurringRule(id), t("accounts.recurringDeleted"))
            }
            onBuyStock={(symbol = "") => {
              setTradeSide("buy");
              setTradeSymbol(symbol);
              setModal("trade");
            }}
            onSellStock={(symbol) => {
              setTradeSide("sell");
              setTradeSymbol(symbol);
              setModal("trade");
            }}
            onSetHoldingPrice={(holdingId, price) =>
              void mutate(() => setHoldingPrice(holdingId, price), t("holdings.updated"))
            }
            onReconcile={(account) => {
              setReconcileAccount(account);
              setModal("reconcile");
            }}
            onRefreshHoldings={() => mutate(() => refreshHoldings(), t("holdings.updated"))}
            onRefreshHolding={(holdingId) => mutate(() => refreshHolding(holdingId), t("holdings.updated"))}
          />
        );
      case "transactions":
        return (
          <TransactionsPage
            data={data}
            onAdd={() => setModal("transaction")}
            onImport={() => setModal("import")}
            onVoid={(transaction) =>
              void mutate(() => voidTransaction(transaction.id), t("transactions.voided"))
            }
            onRestore={(transaction) =>
              void mutate(() => restoreTransaction(transaction.id), t("transactions.restored"))
            }
            onDeletePermanently={(transaction) => {
              const label = transaction.note || transaction.kind;
              if (!window.confirm(t("transactions.confirmDeletePermanent", { label }))) return;
              void mutate(() => deleteTransactionPermanently(transaction.id), t("transactions.deletedPermanently"));
            }}
            onMarkReimbursable={(transaction) =>
              void mutate(() => markReimbursable(transaction.id), t("transactions.markedReimbursable"))
            }
            onUnmarkReimbursable={(transaction) =>
              void mutate(() => unmarkReimbursable(transaction.id), t("transactions.unmarkedReimbursable"))
            }
            onReimburse={(transaction) => {
              setReimburseTarget(transaction);
              setModal("reimburse");
            }}
            onEdit={(transaction) => {
              setEditTransaction(transaction);
              setModal("edit-transaction");
            }}
            onUploadReceipt={(transaction, file) =>
              void mutate(() => uploadReceipt(transaction.id, file), t("transactions.receiptUploaded"))
            }
            onLoadMore={loadMore}
            loadingMore={loadingMore}
            hasMore={txHasMore}
            exportYear={year}
            exportMonth={month}
          />
        );
      case "insights":
        return (
          <InsightsPage
            summary={data.monthly}
            cashFlow={data.cashFlow}
            categories={data.categories}
            budgets={data.budgets}
            onSetBudget={(categoryId, limit) =>
              void mutate(
                () => setBudget(categoryId, data.monthly.year, data.monthly.month, limit),
                t("insights.budget.updated")
              )
            }
            onClearBudget={(categoryId) =>
              void mutate(
                () => clearBudget(categoryId, data.monthly.year, data.monthly.month),
                t("insights.budget.cleared")
              )
            }
          />
        );
      case "users":
        return <UsersAdminPage currentUserId={userId} />;
      case "system":
        return <SystemAdminPage />;
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
          <button className="mobile-close" onClick={() => setMobileNavOpen(false)} aria-label={t("nav.closeMenu")}>
            <X size={20} />
          </button>
        </div>

        <nav className="primary-nav" aria-label={t("nav.main")}>
          {NAV_ITEMS.filter((item) => role === "admin" || (item.id !== "users" && item.id !== "system")).map(({ id, icon: Icon }) => (
            <button
              className={activeView === id ? "active" : ""}
              key={id}
              onClick={() => {
                setActiveView(id);
                setMobileNavOpen(false);
              }}
            >
              <Icon size={19} strokeWidth={1.8} />
              <span>{t(`nav.${id}`)}</span>
            </button>
          ))}
        </nav>

        <div className="sidebar-spacer" />
        <div className="profile-actions">
          <button className="profile-chip" onClick={() => setModal("category")}>
            <span className="avatar">K</span>
            <span>
              <strong>{username}</strong>
            </span>
            <MoreHorizontal size={18} />
          </button>
          <button className="password-button" onClick={() => setModal("password")} aria-label={t("common.changePassword")} title={t("common.changePassword")}><KeyRound size={17} /></button>
          <button className="password-button" onClick={() => setModal("totp")} aria-label={t("totp.title")} title={t("totp.title")}><ShieldCheck size={17} /></button>
          <button className="logout-button" onClick={() => void onLogout()} aria-label={t("common.logout")} title={t("common.logout")}><LogOut size={17} /></button>
        </div>
      </aside>

      {mobileNavOpen && <button className="nav-scrim" onClick={() => setMobileNavOpen(false)} />}

      <main className="main-content">
        <header className="topbar">
          <button className="icon-button mobile-menu" onClick={() => setMobileNavOpen(true)} aria-label={t("nav.openMenu")}>
            <Menu size={20} />
          </button>
          <div className="period-control">
            <CalendarDays size={17} />
            <input
              aria-label={t("topbar.statMonth")}
              type="month"
              value={monthValue}
              disabled={monthValue === ""}
              onChange={(event) => setMonthValue(event.target.value)}
            />
            <button
              type="button"
              className={`all-months-toggle ${monthValue === "" ? "active" : ""}`}
              onClick={() => setMonthValue((value) => (value === "" ? currentMonthValue() : ""))}
              aria-pressed={monthValue === ""}
              title={monthValue === "" ? t("topbar.backToMonthly") : t("topbar.viewAllMonths")}
            >
              {t("topbar.allMonths")}
            </button>
          </div>
          <div className="topbar-actions">
            <label className="currency-select">
              <select aria-label={t("topbar.currencySelect")} value={currency} onChange={(event) => setCurrency(event.target.value)}>
                {currencies.map((item) => (
                  <option key={item}>{item}</option>
                ))}
              </select>
              <ChevronDown size={14} />
            </label>
            <button
              className={`icon-button ${refreshing ? "spinning" : ""}`}
              onClick={() => void refresh(true)}
              aria-label={t("topbar.refresh")}
            >
              <RefreshCcw size={18} />
            </button>
            <div className="reminder-wrap" ref={reminderRef}>
              <button
                className={`icon-button reminder-bell ${reminders.length > 0 ? "has-alerts" : ""}`}
                onClick={() => setReminderOpen((value) => !value)}
                aria-label={t("reminders.title")}
                aria-haspopup="dialog"
                aria-expanded={reminderOpen}
                title={t("reminders.title")}
              >
                <Bell size={18} />
                {reminders.length > 0 && <span className="reminder-count">{reminders.length > 99 ? "99+" : reminders.length}</span>}
              </button>
              {reminderOpen && (
                <div className="reminder-popover" role="dialog" aria-label={t("reminders.title")}>
                  <header>
                    <div><span>REMINDERS</span><strong>{t("reminders.title")}</strong></div>
                    <small>{t("reminders.next30Days")}</small>
                  </header>
                  <div className="reminder-popover-list">
                    {reminders.length === 0 ? (
                      <p className="reminder-empty">{t("reminders.empty")}</p>
                    ) : (
                      reminders.map((item) => (
                        <div className="reminder-item" key={`${item.kind}-${item.id}`}>
                          <div className="reminder-item-main">
                            <strong>{item.title}</strong>
                            <span>{formatMoney(item.amount, item.currency)} · {formatReminderDay(item.due_at)}</span>
                          </div>
                          <span className={`reminder-badge ${item.overdue ? "overdue" : ""}`}>
                            {item.overdue ? t("reminders.overdueDays", { days: Math.abs(item.days_left) }) : t("reminders.daysLeft", { days: item.days_left })}
                          </span>
                        </div>
                      ))
                    )}
                  </div>
                  {role === "admin" && (
                    <div className="reminder-popover-footer">
                      {reminderError && <span className="reminder-error">{reminderError}</span>}
                      <button type="button" className="text-button" onClick={() => void sendDigest()} disabled={reminderSending}>
                        {reminderSending ? <LoaderCircle className="spin" size={14} /> : <Mail size={14} />}
                        {reminderSending ? t("reminders.sending") : t("reminders.sendDigest")}
                      </button>
                    </div>
                  )}
                </div>
              )}
            </div>
            <button
              className="icon-button"
              onClick={() => setTheme(theme === "light" ? "dark" : theme === "dark" ? "system" : "light")}
              aria-label={t("common.themeToggle")}
              title={theme === "light" ? t("common.themeLight") : theme === "dark" ? t("common.themeDark") : t("common.themeSystem")}
            >
              {theme === "light" ? <Sun size={18} /> : theme === "dark" ? <Moon size={18} /> : <Monitor size={18} />}
            </button>
            <button
              className="icon-button"
              onClick={() => void changeLanguage(i18n.language?.toLowerCase().startsWith("en") ? "zh" : "en")}
              aria-label={t("common.language")}
              title={t("common.language")}
            >
              <Globe size={18} />
            </button>
            <button className="primary-button compact" onClick={() => setModal("transaction")}>
              <Plus size={18} />
              <span>{t("common.quickAdd")}</span>
            </button>
          </div>
        </header>

        {error && data && <div className="inline-error">{t("app.syncFailed")}{error}</div>}
        {content}
      </main>

      <nav className="mobile-bottom-nav" aria-label={t("nav.mobile")}>
        {NAV_ITEMS.filter((item) => role === "admin" || (item.id !== "users" && item.id !== "system")).map(({ id, icon: Icon }) => (
          <button key={id} className={activeView === id ? "active" : ""} onClick={() => setActiveView(id)}>
            <Icon size={20} />
            <span>{t(`nav.${id}`)}</span>
          </button>
        ))}
        <button className="mobile-add" onClick={() => setModal("transaction")} aria-label={t("common.quickAdd")}>
          <Plus size={23} />
        </button>
      </nav>

      {modal === "transaction" && data && (
        <TransactionModal
          accounts={data.accounts}
          categories={data.categories}
          tags={data.tags}
          onClose={() => setModal(null)}
          onSubmit={(input) =>
            mutate(
              () =>
                input.kind === "transfer"
                  ? createTransfer(input.payload)
                  : createTransaction(input.payload),
              input.kind === "transfer" ? t("transactions.transferDone") : t("transactions.recorded")
            )
          }
        />
      )}
      {modal === "account" && (
        <AccountModal
          currencies={currencies}
          onClose={() => setModal(null)}
          onSubmit={(input) => mutate(() => createAccount(input), t("accounts.created"))}
        />
      )}
      {modal === "edit-account" && data && editAccount && (
        <EditAccountModal
          account={editAccount}
          currencies={currencies}
          onClose={() => setModal(null)}
          onSubmit={(input) =>
            mutate(
              async () => {
                await updateAccount(editAccount.id, input.details);
                if (input.adjustment !== undefined) {
                  await adjustBalance(editAccount.id, { amount: input.adjustment, note: t("accounts.balanceAdjustment") });
                }
              },
              t("accounts.updated")
            )
          }
        />
      )}
      {modal === "category" && (
        <CategoryModal
          categories={data?.categories ?? []}
          onClose={() => setModal(null)}
          onSubmit={(input) => mutate(() => createCategory(input), t("modals.category.created"))}
          onDelete={async (category) => {
            await deleteCategory(category.id);
            setToast(t("modals.category.deleted", { name: category.name }));
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
              t("deposit.converted")
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
              t("deposit.settled")
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
              t("transactions.reimbursed")
            )
          }
        />
      )}
      {modal === "loan" && data && (
        <LoanModal
          accounts={data.accounts}
          counterparties={[...new Set(data.loans.map((loan) => loan.counterparty))]}
          onClose={() => setModal(null)}
          onSubmit={(input) => mutate(() => createLoan(input), t("accounts.loanRecorded"))}
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
              t("accounts.repaid")
            )
          }
        />
      )}
      {modal === "recurring" && data && (
        <RecurringModal
          accounts={data.accounts}
          categories={data.categories}
          onClose={() => setModal(null)}
          onSubmit={(input) => mutate(() => createRecurringRule(input), t("accounts.recurringCreated"))}
        />
      )}
      {modal === "trade" && data && (
        <TradeModal
          accounts={data.accounts}
          initialSide={tradeSide}
          initialSymbol={tradeSymbol}
          onClose={() => setModal(null)}
          onSubmit={(input) =>
            mutate(
              () => (input.side === "buy" ? buyStock(input.payload) : sellStock(input.payload)),
              input.side === "buy" ? t("holdings.buyRecorded") : t("holdings.sellRecorded")
            )
          }
        />
      )}
      {modal === "password" && (
        <PasswordModal
          onClose={() => setModal(null)}
          onSubmit={async (oldPassword, newPassword) => {
            await changePassword(oldPassword, newPassword);
            setModal(null);
            await onLogout();
          }}
        />
      )}
      {modal === "totp" && (
        <TotpModal
          onClose={() => setModal(null)}
        />
      )}
      {modal === "reconcile" && data && reconcileAccount && (
        <ReconciliationModal
          account={reconcileAccount}
          onClose={() => setModal(null)}
          onChanged={() => void mutate(async () => undefined, t("reconcile.done"))}
        />
      )}
      {modal === "import" && data && (
        <ImportModal
          accounts={data.accounts}
          categories={data.categories}
          onClose={() => setModal(null)}
          onComplete={() => void mutate(async () => undefined, t("modals.import.done"))}
        />
      )}
      {modal === "edit-transaction" && data && editTransaction && (
        <EditTransactionModal
          transaction={editTransaction}
          accounts={data.accounts}
          categories={data.categories}
          tags={data.tags}
          onClose={() => setModal(null)}
          onSubmit={(input) =>
            mutate(
              () => updateTransaction(editTransaction.id, input),
              t("transactions.updated")
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

function AuthLoadingState() {
  const { t } = useTranslation();
  return <main className="auth-loading"><div className="loading-mark"><span /><span /></div><p>{t("app.checkingSession")}</p></main>;
}

function LoadingState() {
  const { t } = useTranslation();
  return <div className="loading-state"><div className="loading-mark"><span /><span /></div><p>{t("app.loadingLedger")}</p></div>;
}

function ErrorState({ message, onRetry }: { message: string; onRetry: () => void }) {
  const { t } = useTranslation();
  return <div className="error-state"><CircleDollarSign size={34} /><h2>{t("app.errorTitle")}</h2><p>{message}</p><button className="primary-button" onClick={onRetry}><RefreshCcw size={17} />{t("app.reconnect")}</button></div>;
}
