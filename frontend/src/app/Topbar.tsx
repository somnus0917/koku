//! 顶栏：月份/币种切换、刷新、提醒铃铛、主题与语言。
import { useTranslation } from "react-i18next";
import {
  Bell,
  CalendarDays,
  ChevronDown,
  Globe,
  LoaderCircle,
  Mail,
  Menu,
  Monitor,
  Moon,
  Plus,
  RefreshCcw,
  Sun
} from "lucide-react";
import { formatMoney } from "../lib";
import { uiLocale } from "../i18n";
import type { RefObject } from "react";
import type { ReminderItem, UserRole } from "../types";
import type { ThemePreference } from "../theme";

/** 提醒到期日展示：YYYY-MM-DD / RFC3339 → "8月20日"（随界面语言变化）。 */
function formatReminderDay(value: string): string {
  const date = /^\d{4}-\d{2}-\d{2}$/.test(value) ? new Date(`${value}T00:00:00`) : new Date(value);
  return new Intl.DateTimeFormat(uiLocale(), { month: "long", day: "numeric" }).format(date);
}

export function Topbar({
  monthValue,
  onMonthValueChange,
  onToggleAllMonths,
  currency,
  currencies,
  onCurrencyChange,
  refreshing,
  onRefresh,
  reminders,
  reminderOpen,
  onToggleReminder,
  reminderRef,
  reminderError,
  reminderSending,
  onSendDigest,
  onReminderAction,
  onOpenMobileMenu,
  role,
  theme,
  onToggleTheme,
  onLanguageToggle,
  onQuickAdd
}: {
  monthValue: string;
  onMonthValueChange: (value: string) => void;
  onToggleAllMonths: () => void;
  currency: string;
  currencies: string[];
  onCurrencyChange: (value: string) => void;
  refreshing: boolean;
  onRefresh: () => void;
  reminders: ReminderItem[];
  reminderOpen: boolean;
  onToggleReminder: () => void;
  reminderRef: RefObject<HTMLDivElement | null>;
  reminderError: string | null;
  reminderSending: boolean;
  onSendDigest: () => void;
  onReminderAction: (item: ReminderItem) => void;
  onOpenMobileMenu: () => void;
  role: UserRole;
  theme: ThemePreference;
  onToggleTheme: () => void;
  onLanguageToggle: () => void;
  onQuickAdd: () => void;
}) {
  const { t } = useTranslation();
  return (
    <header className="topbar">
      <button className="icon-button mobile-menu" onClick={onOpenMobileMenu} aria-label={t("nav.openMenu")}>
        <Menu size={20} />
      </button>
      <div className="period-control">
        <CalendarDays size={17} />
        <input
          aria-label={t("topbar.statMonth")}
          type="month"
          value={monthValue}
          disabled={monthValue === ""}
          onChange={(event) => onMonthValueChange(event.target.value)}
        />
        <button
          type="button"
          className={`all-months-toggle ${monthValue === "" ? "active" : ""}`}
          onClick={onToggleAllMonths}
          aria-pressed={monthValue === ""}
          title={monthValue === "" ? t("topbar.backToMonthly") : t("topbar.viewAllMonths")}
        >
          {t("topbar.allMonths")}
        </button>
      </div>
      <div className="topbar-actions">
        <label className="currency-select">
          <select aria-label={t("topbar.currencySelect")} value={currency} onChange={(event) => onCurrencyChange(event.target.value)}>
            {currencies.map((item) => (
              <option key={item}>{item}</option>
            ))}
          </select>
          <ChevronDown size={14} />
        </label>
        <button
          className={`icon-button ${refreshing ? "spinning" : ""}`}
          onClick={onRefresh}
          aria-label={t("topbar.refresh")}
        >
          <RefreshCcw size={18} />
        </button>
        <div className="reminder-wrap" ref={reminderRef}>
          <button
            className={`icon-button reminder-bell ${reminders.length > 0 ? "has-alerts" : ""}`}
            onClick={onToggleReminder}
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
                    <button className="reminder-item" type="button" key={`${item.kind}-${item.id}`} onClick={() => onReminderAction(item)}>
                      <div className="reminder-item-main">
                        <strong>{item.title}</strong>
                        <span>{formatMoney(item.amount, item.currency)} · {formatReminderDay(item.due_at)}</span>
                      </div>
                      <span className={`reminder-badge ${item.overdue ? "overdue" : ""}`}>
                        {item.overdue ? t("reminders.overdueDays", { days: Math.abs(item.days_left) }) : t("reminders.daysLeft", { days: item.days_left })}
                      </span>
                    </button>
                  ))
                )}
              </div>
              {role === "admin" && (
                <div className="reminder-popover-footer">
                  {reminderError && <span className="reminder-error">{reminderError}</span>}
                  <button type="button" className="text-button" onClick={onSendDigest} disabled={reminderSending}>
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
          onClick={onToggleTheme}
          aria-label={t("common.themeToggle")}
          title={theme === "light" ? t("common.themeLight") : theme === "dark" ? t("common.themeDark") : t("common.themeSystem")}
        >
          {theme === "light" ? <Sun size={18} /> : theme === "dark" ? <Moon size={18} /> : <Monitor size={18} />}
        </button>
        <button
          className="icon-button"
          onClick={onLanguageToggle}
          aria-label={t("common.language")}
          title={t("common.language")}
        >
          <Globe size={18} />
        </button>
        <button className="primary-button compact" onClick={onQuickAdd}>
          <Plus size={18} />
          <span>{t("common.quickAdd")}</span>
        </button>
      </div>
    </header>
  );
}
