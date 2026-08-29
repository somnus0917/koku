//! 主应用外壳：数据加载、页面路由、弹窗编排与全局通知。
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, CircleDollarSign, RefreshCcw } from "lucide-react";
import { changeLanguage } from "../i18n";
import {
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
  deleteTransactionPermanently,
  loadReminders,
  loadSummaryData,
  loadTransactions,
  markReimbursable,
  refreshHoldings,
  refreshHolding,
  reimburse,
  refund,
  repayLoan,
  restoreTransaction,
  sellStock,
  sendReminderDigest,
  setBudget,
  setHoldingPrice,
  setRecurringPaused,
  settleDeposit,
  unmarkReimbursable,
  updateAccount,
  updateRecurringRule,
  updateTransaction,
  uploadReceipt,
  voidTransaction
} from "../api";
import { AccountModal } from "../features/accounts/AccountModal";
import { EditAccountModal } from "../features/accounts/EditAccountModal";
import { PasswordModal } from "../features/auth/PasswordModal";
import { TotpModal } from "../features/auth/TotpModal";
import { LearningSettingsModal } from "../features/settings/LearningSettingsModal";
import { CategoryModal } from "../features/categories/CategoryModal";
import { DepositModal } from "../features/deposits/DepositModal";
import { SettleDepositModal } from "../features/deposits/SettleDepositModal";
import { TradeModal } from "../features/holdings/TradeModal";
import { LedgerSettingsModal } from "../features/settings/LedgerSettingsModal";
import { LoanModal } from "../features/loans/LoanModal";
import { RepayModal } from "../features/loans/RepayModal";
import { ReimburseModal } from "../features/reimbursements/ReimburseModal";
import { RefundModal } from "../features/refunds/RefundModal";
import { ReconciliationModal } from "../features/reconciliation/ReconciliationModal";
import { RecurringModal } from "../features/recurring/RecurringModal";
import { EditTransactionModal } from "../features/transactions/EditTransactionModal";
import { ImportModal } from "../features/transactions/ImportModal";
import { TransactionModal } from "../features/transactions/TransactionModal";
import { COMMON_CURRENCIES, availableCurrencies, currentMonthValue } from "../lib";
import { useTheme } from "../theme";
import { MobileBottomNav } from "./MobileBottomNav";
import { Sidebar } from "./Sidebar";
import { Topbar } from "./Topbar";
import type { View } from "./nav";
import type { Account, AppData, Deposit, Loan, ReminderItem, RecurringRule, Transaction, UserRole } from "../types";

const Dashboard = lazy(() => import("../features/dashboard/DashboardPage").then((module) => ({ default: module.Dashboard })));
const TasksPage = lazy(() => import("../features/tasks/TasksPage").then((module) => ({ default: module.TasksPage })));
const AccountsPage = lazy(() => import("../features/accounts/AccountsPage").then((module) => ({ default: module.AccountsPage })));
const TransactionsPage = lazy(() => import("../features/transactions/TransactionsPage").then((module) => ({ default: module.TransactionsPage })));
const InsightsPage = lazy(() => import("../features/insights/InsightsPage").then((module) => ({ default: module.InsightsPage })));
const ActivityPage = lazy(() => import("../features/activity/ActivityPage").then((module) => ({ default: module.ActivityPage })));
const UsersAdminPage = lazy(() => import("../components/users").then((module) => ({ default: module.UsersAdminPage })));
const SystemAdminPage = lazy(() => import("../components/system").then((module) => ({ default: module.SystemAdminPage })));

type Modal =
  | "transaction"
  | "account"
  | "category"
  | "deposit"
  | "settle"
  | "reimburse"
  | "refund"
  | "loan"
  | "repay"
  | "edit-account"
  | "edit-transaction"
  | "recurring"
  | "trade"
  | "password"
  | "import"
  | "totp"
  | "reconcile"
  | "settings"
  | "ledger-settings"
  | null;

/** 「全部月份」模式下的分页大小；单月模式一次性取上限（单月很少超过）。 */
const TRANSACTIONS_PAGE_SIZE = 200;
const MONTH_TRANSACTIONS_LIMIT = 1000;
type TransactionFilters = { search: string; kind: string; tags: string[] };

function sameTransactionFilters(left: TransactionFilters, right: TransactionFilters) {
  return left.search === right.search
    && left.kind === right.kind
    && left.tags.length === right.tags.length
    && left.tags.every((tag, index) => tag === right.tags[index]);
}

export function LedgerApp({ username, role, userId, onLogout }: { username: string; role: UserRole; userId: number; onLogout: () => Promise<void> }) {
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
  const [refundTarget, setRefundTarget] = useState<Transaction | null>(null);
  const [loanTarget, setLoanTarget] = useState<Loan | null>(null);
  const [editRecurring, setEditRecurring] = useState<RecurringRule | null>(null);
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
  const [txFilters, setTxFilters] = useState<TransactionFilters>({ search: "", kind: "all", tags: [] });
  const [mutationError, setMutationError] = useState<string | null>(null);
  const [ledgerRevision, setLedgerRevision] = useState(0);
  const txFiltersRef = useRef(txFilters);
  const summaryRequestRef = useRef(0);
  const transactionRequestRef = useRef(0);
  const mutationInFlightRef = useRef(false);
  txFiltersRef.current = txFilters;

  // monthValue 为空字符串表示「全部月份」；此时 year/month 为 undefined。
  const monthParts = monthValue ? monthValue.split("-").map(Number) : [];
  const year = monthParts[0];
  const month = monthParts[1];

  const refresh = useCallback(
    async (quiet = false) => {
      const summaryRequest = ++summaryRequestRef.current;
      const transactionRequest = ++transactionRequestRef.current;
      if (quiet) setRefreshing(true);
      else setLoading(true);
      try {
        // 周期交易与预算结转由服务端后台任务执行；前端只读取最新账本状态。
        const now = new Date();
        const summaryYear = year ?? now.getFullYear();
        const summaryMonth = month ?? now.getMonth() + 1;
        const limit = month === undefined ? TRANSACTIONS_PAGE_SIZE : MONTH_TRANSACTIONS_LIMIT;
        const [summary, transactions] = await Promise.all([
          loadSummaryData(summaryYear, summaryMonth, currency),
          loadTransactions(0, limit, year, month, txFiltersRef.current)
        ]);
        if (summaryRequest !== summaryRequestRef.current) return;
        const transactionsAreCurrent = transactionRequest === transactionRequestRef.current;
        setData((current) => ({
          ...summary,
          transactions: transactionsAreCurrent ? transactions : current?.transactions ?? transactions
        }));
        if (transactionsAreCurrent) {
          setTxOffset(transactions.length);
          setTxHasMore(month === undefined && transactions.length === TRANSACTIONS_PAGE_SIZE);
        }
        setLedgerRevision((revision) => revision + 1);
        setError(null);
      } catch (reason) {
        if (summaryRequest === summaryRequestRef.current) {
          setError(reason instanceof Error ? reason.message : t("app.apiUnreachable"));
        }
      } finally {
        if (summaryRequest === summaryRequestRef.current) {
          setLoading(false);
          setRefreshing(false);
        }
      }
    },
    [currency, month, year, t]
  );

  const refreshTransactions = useCallback(async () => {
    const request = ++transactionRequestRef.current;
    const limit = month === undefined ? TRANSACTIONS_PAGE_SIZE : MONTH_TRANSACTIONS_LIMIT;
    try {
      const transactions = await loadTransactions(0, limit, year, month, txFilters);
      if (request !== transactionRequestRef.current) return;
      setData((current) => current ? { ...current, transactions } : current);
      setTxOffset(transactions.length);
      setTxHasMore(month === undefined && transactions.length === TRANSACTIONS_PAGE_SIZE);
      setError(null);
    } catch (reason) {
      if (request === transactionRequestRef.current) {
        setError(reason instanceof Error ? reason.message : t("app.apiUnreachable"));
      }
    }
  }, [month, t, txFilters, year]);

  const loadMore = useCallback(async () => {
    if (loadingMore || !txHasMore) return;
    const request = transactionRequestRef.current;
    setLoadingMore(true);
    try {
      const page = await loadTransactions(txOffset, TRANSACTIONS_PAGE_SIZE, year, month, txFilters);
      if (request !== transactionRequestRef.current) return;
      setData((current) =>
        current ? { ...current, transactions: [...current.transactions, ...page] } : current
      );
      setTxOffset((offset) => offset + page.length);
      setTxHasMore(page.length === TRANSACTIONS_PAGE_SIZE);
    } catch (reason) {
      if (request === transactionRequestRef.current) {
        setError(reason instanceof Error ? reason.message : t("app.loadMoreFailed"));
      }
    } finally {
      setLoadingMore(false);
    }
  }, [loadingMore, txHasMore, txOffset, t, year, month, txFilters]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const previousFilters = useRef(txFilters);
  useEffect(() => {
    if (sameTransactionFilters(previousFilters.current, txFilters)) return;
    previousFilters.current = txFilters;
    void refreshTransactions();
  }, [refreshTransactions, txFilters]);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 2600);
    return () => window.clearTimeout(timer);
  }, [toast]);

  // 提醒铃铛：数据每次刷新完成后重新拉取（未来 30 天到期项），失败静默。
  useEffect(() => {
    let cancelled = false;
    loadReminders(30, currency)
      .then((items) => {
        if (!cancelled) setReminders(items);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [ledgerRevision, currency]);

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
    if (mutationInFlightRef.current) {
      throw new Error(t("common.opInProgress"));
    }
    mutationInFlightRef.current = true;
    setMutationError(null);
    try {
      await action();
      setModal(null);
      setToast(successMessage);
      await refresh(true);
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : t("common.opFailed");
      setMutationError(message);
      throw reason instanceof Error ? reason : new Error(message);
    } finally {
      mutationInFlightRef.current = false;
    }
  };

  const runMutation = (action: () => Promise<unknown>, successMessage: string) => {
    void mutate(action, successMessage).catch(() => undefined);
  };

  const sendDigest = async () => {
    setReminderSending(true);
    setReminderError(null);
    try {
      const result = await sendReminderDigest(currency);
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
      case "tasks":
        return <TasksPage reminders={reminders} onOpen={(item) => {
          if (item.kind === "deposit") {
            const deposit = data.deposits.find((candidate) => candidate.id === item.id);
            if (deposit) {
              setSettleTarget(deposit);
              setModal("settle");
              return;
            }
          }
          if (item.kind === "loan") {
            const loan = data.loans.find((candidate) => candidate.id === item.id);
            if (loan) {
              setLoanTarget(loan);
              setModal("repay");
              return;
            }
          }
          if (item.kind === "savings_goal") {
            setActiveView("dashboard");
            return;
          }
          if (item.kind === "budget") {
            setActiveView("insights");
            return;
          }
          setActiveView("accounts");
        }} />;
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
            onCreateRecurring={() => { setEditRecurring(null); setModal("recurring"); }}
            onOpenLedgerSettings={() => setModal("ledger-settings")}
            onDeleteRecurring={(id) =>
              runMutation(() => deleteRecurringRule(id), t("accounts.recurringDeleted"))
            }
            onEditRecurring={(rule) => { setEditRecurring(rule); setModal("recurring"); }}
            onToggleRecurringPaused={(rule) =>
              runMutation(() => setRecurringPaused(rule.id, !rule.paused_at), rule.paused_at ? t("accounts.recurringResumed") : t("accounts.recurringPaused"))
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
              runMutation(() => setHoldingPrice(holdingId, price), t("holdings.updated"))
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
              runMutation(() => voidTransaction(transaction.id), t("transactions.voided"))
            }
            onRestore={(transaction) =>
              runMutation(() => restoreTransaction(transaction.id), t("transactions.restored"))
            }
            onDeletePermanently={(transaction) => {
              const label = transaction.note || transaction.kind;
              if (!window.confirm(t("transactions.confirmDeletePermanent", { label }))) return;
              runMutation(() => deleteTransactionPermanently(transaction.id), t("transactions.deletedPermanently"));
            }}
            onMarkReimbursable={(transaction) =>
              runMutation(() => markReimbursable(transaction.id), t("transactions.markedReimbursable"))
            }
            onUnmarkReimbursable={(transaction) =>
              runMutation(() => unmarkReimbursable(transaction.id), t("transactions.unmarkedReimbursable"))
            }
            onReimburse={(transaction) => {
              setReimburseTarget(transaction);
              setModal("reimburse");
            }}
            onRefund={(transaction) => {
              setRefundTarget(transaction);
              setModal("refund");
            }}
            onEdit={(transaction) => {
              setEditTransaction(transaction);
              setModal("edit-transaction");
            }}
            onUploadReceipt={(transaction, file) =>
              runMutation(() => uploadReceipt(transaction.id, file), t("transactions.receiptUploaded"))
            }
            onLoadMore={loadMore}
            loadingMore={loadingMore}
            hasMore={txHasMore}
            onFilterChange={(filters) => setTxFilters((current) =>
              sameTransactionFilters(current, filters) ? current : filters
            )}
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
              runMutation(
                () => setBudget(categoryId, data.monthly.year, data.monthly.month, limit),
                t("insights.budget.updated")
              )
            }
            onClearBudget={(categoryId) =>
              runMutation(
                () => clearBudget(categoryId, data.monthly.year, data.monthly.month),
                t("insights.budget.cleared")
              )
            }
          />
        );
      case "activity":
        return <ActivityPage />;
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
      <Sidebar
        username={username}
        role={role}
        activeView={activeView}
        onNavigate={(view) => {
          setActiveView(view);
          setMobileNavOpen(false);
        }}
        onOpenCategories={() => setModal("category")}
        onOpenPassword={() => setModal("password")}
        onOpenTotp={() => setModal("totp")}
        onOpenLearningSettings={() => setModal("settings")}
        onLogout={() => void onLogout()}
        mobileNavOpen={mobileNavOpen}
        onCloseMobileNav={() => setMobileNavOpen(false)}
      />

      {mobileNavOpen && <button className="nav-scrim" onClick={() => setMobileNavOpen(false)} />}

      <main className="main-content">
        <Topbar
          monthValue={monthValue}
          onMonthValueChange={setMonthValue}
          onToggleAllMonths={() => setMonthValue((value) => (value === "" ? currentMonthValue() : ""))}
          currency={currency}
          currencies={currencies}
          onCurrencyChange={setCurrency}
          refreshing={refreshing}
          onRefresh={() => void refresh(true)}
          reminders={reminders}
          reminderOpen={reminderOpen}
          onToggleReminder={() => setReminderOpen((value) => !value)}
          reminderRef={reminderRef}
          reminderError={reminderError}
          reminderSending={reminderSending}
          onSendDigest={() => void sendDigest()}
          onReminderAction={(item) => {
            setReminderOpen(false);
            setActiveView(item.kind === "savings_goal" ? "dashboard" : item.kind === "budget" ? "insights" : "accounts");
          }}
          onOpenMobileMenu={() => setMobileNavOpen(true)}
          role={role}
          theme={theme}
          onToggleTheme={() => setTheme(theme === "light" ? "dark" : theme === "dark" ? "system" : "light")}
          onLanguageToggle={() => void changeLanguage(i18n.language?.toLowerCase().startsWith("en") ? "zh" : "en")}
          onQuickAdd={() => setModal("transaction")}
        />

        {error && data && <div className="inline-error">{t("app.syncFailed")}{error}</div>}
        {mutationError && <div className="inline-error" role="alert">{mutationError}</div>}
        <Suspense fallback={<div className="page page-enter" aria-hidden="true" />}>
          {content}
        </Suspense>
      </main>
      <MobileBottomNav
        activeView={activeView}
        onNavigate={setActiveView}
        onQuickAdd={() => setModal("transaction")}
      />

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
      {modal === "ledger-settings" && data && (
        <LedgerSettingsModal accounts={data.accounts} categories={data.categories} onClose={() => setModal(null)} />
      )}
      {modal === "edit-account" && data && editAccount && (
        <EditAccountModal
          account={editAccount}
          currencies={currencies}
          onClose={() => setModal(null)}
          onSubmit={(input) =>
            mutate(
              async () => {
                await updateAccount(editAccount.id, {
                  ...input.details,
                  balance_adjustment: input.adjustment,
                  adjustment_note: input.adjustment === undefined ? undefined : t("accounts.balanceAdjustment")
                });
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
      {modal === "refund" && data && refundTarget && (
        <RefundModal
          expense={refundTarget}
          accounts={data.accounts}
          onClose={() => setModal(null)}
          onSubmit={(input) =>
            mutate(
              () => refund({ expense_id: refundTarget.id, ...input }),
              t("transactions.refunded")
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
          rule={editRecurring}
          onClose={() => setModal(null)}
          onSubmit={(input) => mutate(() => editRecurring ? updateRecurringRule(editRecurring.id, input) : createRecurringRule(input), t("accounts.recurringCreated"))}
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
      {modal === "settings" && (
        <LearningSettingsModal
          onClose={() => setModal(null)}
        />
      )}
      {modal === "reconcile" && data && reconcileAccount && (
        <ReconciliationModal
          account={reconcileAccount}
          onClose={() => setModal(null)}
          onChanged={() => runMutation(async () => undefined, t("reconcile.done"))}
        />
      )}
      {modal === "import" && data && (
        <ImportModal
          accounts={data.accounts}
          categories={data.categories}
          onClose={() => setModal(null)}
          onComplete={() => runMutation(async () => undefined, t("modals.import.done"))}
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


function LoadingState() {
  const { t } = useTranslation();
  return <div className="loading-state"><div className="loading-mark"><span /><span /></div><p>{t("app.loadingLedger")}</p></div>;
}


function ErrorState({ message, onRetry }: { message: string; onRetry: () => void }) {
  const { t } = useTranslation();
  return <div className="error-state"><CircleDollarSign size={34} /><h2>{t("app.errorTitle")}</h2><p>{message}</p><button className="primary-button" onClick={onRetry}><RefreshCcw size={17} />{t("app.reconnect")}</button></div>;
}
