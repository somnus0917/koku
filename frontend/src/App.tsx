import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type CSSProperties,
  type FormEvent
} from "react";
import {
  CalendarDays,
  ChartNoAxesCombined,
  Check,
  ChevronDown,
  CircleDollarSign,
  Eye,
  EyeOff,
  LayoutDashboard,
  LoaderCircle,
  LockKeyhole,
  LogOut,
  Menu,
  Moon,
  MoreHorizontal,
  Plus,
  ReceiptText,
  RefreshCcw,
  ShieldCheck,
  Sun,
  WalletCards,
  X,
  type LucideIcon
} from "lucide-react";
import {
  ApiError,
  adjustBalance,
  buyStock,
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
  loadSummaryData,
  loadTransactions,
  login,
  logout,
  markReimbursable,
  reimburse,
  repayLoan,
  runRecurring,
  sellStock,
  setBudget,
  setHoldingPrice,
  settleDeposit,
  unmarkReimbursable,
  updateAccount,
  updateTransaction,
  uploadReceipt,
  voidTransaction
} from "./api";
import {
  AccountModal,
  CategoryModal,
  DepositModal,
  EditAccountModal,
  EditTransactionModal,
  LoanModal,
  RecurringModal,
  ReimburseModal,
  RepayModal,
  SettleDepositModal,
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
  Loan,
  LoanType,
  MonthlySummary,
  Transaction,
  TransactionKind
} from "./types";

type View = "dashboard" | "accounts" | "transactions" | "insights";
type Modal = "transaction" | "account" | "category" | "deposit" | "settle" | "reimburse" | "loan" | "repay" | "edit-account" | "edit-transaction" | "recurring" | "trade" | null;

const NAV_ITEMS: Array<{ id: View; label: string; icon: LucideIcon }> = [
  { id: "dashboard", label: "总览", icon: LayoutDashboard },
  { id: "accounts", label: "账户", icon: WalletCards },
  { id: "transactions", label: "交易", icon: ReceiptText },
  { id: "insights", label: "分析", icon: ChartNoAxesCombined }
];

/** 「全部月份」模式下的分页大小；单月模式一次性取上限（单月很少超过）。 */
const TRANSACTIONS_PAGE_SIZE = 200;
const MONTH_TRANSACTIONS_LIMIT = 1000;

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
  const [editAccount, setEditAccount] = useState<Account | null>(null);
  const [editTransaction, setEditTransaction] = useState<Transaction | null>(null);
  const [depositFrom, setDepositFrom] = useState<Account | null>(null);
  const [settleTarget, setSettleTarget] = useState<Account | null>(null);
  const [reimburseTarget, setReimburseTarget] = useState<Transaction | null>(null);
  const [loanTarget, setLoanTarget] = useState<Loan | null>(null);
  const [tradeSide, setTradeSide] = useState<"buy" | "sell">("buy");
  const [tradeSymbol, setTradeSymbol] = useState("");
  const [toast, setToast] = useState<string | null>(null);
  const [mobileNavOpen, setMobileNavOpen] = useState(false);
  const [dark, setDark] = useState(() => localStorage.getItem("koku-theme") === "dark");
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
        // 先触发周期交易到期生成（请求驱动、无后台任务），再读取最新数据。
        await runRecurring().catch(() => undefined);
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
        setError(reason instanceof Error ? reason.message : "无法连接 Koku API");
      } finally {
        setLoading(false);
        setRefreshing(false);
      }
    },
    [currency, month, year]
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
      setError(reason instanceof Error ? reason.message : "无法加载更多交易");
    } finally {
      setLoadingMore(false);
    }
  }, [loadingMore, txHasMore, txOffset]);

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
            onEdit={(account) => {
              setEditAccount(account);
              setModal("edit-account");
            }}
            onDeposit={(account) => {
              setDepositFrom(account);
              setModal("deposit");
            }}
            onSettle={(account) => {
              setSettleTarget(account);
              setModal("settle");
            }}
            onCreateLoan={() => setModal("loan")}
            onRepay={(loan) => {
              setLoanTarget(loan);
              setModal("repay");
            }}
            onCreateRecurring={() => setModal("recurring")}
            onDeleteRecurring={(id) =>
              void mutate(() => deleteRecurringRule(id), "周期交易已删除")
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
              void mutate(() => setHoldingPrice(holdingId, price), "已更新市价")
            }
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
            onUnmarkReimbursable={(transaction) =>
              void mutate(() => unmarkReimbursable(transaction.id), "已取消待报销标记")
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
              void mutate(() => uploadReceipt(transaction.id, file), "小票已上传")
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
                "预算已更新"
              )
            }
            onClearBudget={(categoryId) =>
              void mutate(
                () => clearBudget(categoryId, data.monthly.year, data.monthly.month),
                "预算已清除"
              )
            }
          />
        );
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
        <div className="profile-actions">
          <button className="profile-chip" onClick={() => setModal("category")}>
            <span className="avatar">K</span>
            <span>
              <strong>{username}</strong>
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
              disabled={monthValue === ""}
              onChange={(event) => setMonthValue(event.target.value)}
            />
            <button
              type="button"
              className={`all-months-toggle ${monthValue === "" ? "active" : ""}`}
              onClick={() => setMonthValue((value) => (value === "" ? currentMonthValue() : ""))}
              aria-pressed={monthValue === ""}
              title={monthValue === "" ? "切回按月查看" : "查看全部月份（交易列表分页加载）"}
            >
              全部
            </button>
          </div>
          <div className="topbar-actions">
            <label className="currency-select">
              <select aria-label="显示币种（按当前汇率折算所有金额）" value={currency} onChange={(event) => setCurrency(event.target.value)}>
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
          tags={data.tags}
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
          currencies={currencies}
          onClose={() => setModal(null)}
          onSubmit={(input) => mutate(() => createAccount(input), "账户已创建")}
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
                  await adjustBalance(editAccount.id, { amount: input.adjustment, note: "余额调整" });
                }
              },
              "账户已更新"
            )
          }
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
          counterparties={[...new Set(data.loans.map((loan) => loan.counterparty))]}
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
      {modal === "recurring" && data && (
        <RecurringModal
          accounts={data.accounts}
          categories={data.categories}
          onClose={() => setModal(null)}
          onSubmit={(input) => mutate(() => createRecurringRule(input), "周期交易已创建")}
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
              input.side === "buy" ? "买入已记录" : "卖出已记录"
            )
          }
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
              "交易已更新"
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
  return <main className="auth-loading"><div className="loading-mark"><span /><span /></div><p>正在验证安全会话…</p></main>;
}

function LoadingState() {
  return <div className="loading-state"><div className="loading-mark"><span /><span /></div><p>正在打开你的账本…</p></div>;
}

function ErrorState({ message, onRetry }: { message: string; onRetry: () => void }) {
  return <div className="error-state"><CircleDollarSign size={34} /><h2>暂时无法读取账本</h2><p>{message}</p><button className="primary-button" onClick={onRetry}><RefreshCcw size={17} />重新连接</button></div>;
}
