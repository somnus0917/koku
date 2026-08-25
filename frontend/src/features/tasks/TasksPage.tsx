//! 财务待办工作台：将账本中的到期事实按紧急程度组织成可处理的任务卡。
import { AlertTriangle, CalendarClock, CircleCheck, Gauge, Landmark, PiggyBank, ReceiptText, WalletCards } from "lucide-react";
import { useTranslation } from "react-i18next";
import { EmptyState } from "../../components/EmptyState";
import { PageTitle } from "../../components/PageTitle";
import { formatMoney } from "../../lib";
import type { ReminderItem } from "../../types";

type Lane = "urgent" | "soon" | "planned";

const laneFor = (item: ReminderItem): Lane => item.overdue || item.days_left <= 2 ? "urgent" : item.days_left <= 7 ? "soon" : "planned";

function iconFor(kind: ReminderItem["kind"]) {
  if (kind === "deposit") return Landmark;
  if (kind === "credit_card") return WalletCards;
  if (kind === "bill") return CalendarClock;
  if (kind === "savings_goal") return PiggyBank;
  if (kind === "budget") return Gauge;
  return ReceiptText;
}

function TaskCard({ item, onOpen }: { item: ReminderItem; onOpen: (item: ReminderItem) => void }) {
  const { t } = useTranslation();
  const Icon = iconFor(item.kind);
  const status = item.kind === "budget"
    ? t(item.overdue ? "tasks.budgetOver" : "tasks.budgetNear")
    : t(item.overdue ? "tasks.overdue" : "tasks.dueIn", { days: item.days_left });
  return (
    <article className={`financial-task-card ${item.overdue ? "overdue" : ""}`}>
      <span className="financial-task-icon"><Icon size={18} /></span>
      <div className="financial-task-copy">
        <small>{t(`tasks.kind.${item.kind}`)}</small>
        <strong>{item.title}</strong>
        <span>{formatMoney(item.amount, item.currency)} · {status}{item.progress_percent !== undefined ? ` · ${t("tasks.progress", { percent: item.progress_percent })}` : ""}</span>
      </div>
      <button type="button" className="text-button" onClick={() => onOpen(item)}>{t(`tasks.action.${item.kind}`)}</button>
    </article>
  );
}

export function TasksPage({ reminders, onOpen }: { reminders: ReminderItem[]; onOpen: (item: ReminderItem) => void }) {
  const { t } = useTranslation();
  const lanes: Array<{ id: Lane; icon: typeof AlertTriangle; items: ReminderItem[] }> = [
    { id: "urgent", icon: AlertTriangle, items: reminders.filter((item) => laneFor(item) === "urgent") },
    { id: "soon", icon: CalendarClock, items: reminders.filter((item) => laneFor(item) === "soon") },
    { id: "planned", icon: CircleCheck, items: reminders.filter((item) => laneFor(item) === "planned") }
  ];
  return (
    <div className="page page-enter">
      <PageTitle eyebrow="FINANCIAL WORKBENCH" title={t("tasks.title")} />
      <p className="page-hint">{t("tasks.hint")}</p>
      {reminders.length === 0 ? (
        <EmptyState title={t("tasks.emptyTitle")} detail={t("tasks.emptyDetail")} />
      ) : (
        <div className="financial-task-board">
          {lanes.map(({ id, icon: Icon, items }) => (
            <section className={`financial-task-lane ${id}`} key={id} aria-label={t(`tasks.lane.${id}`)}>
              <header><span><Icon size={16} /></span><div><small>{t(`tasks.laneLabel.${id}`)}</small><h2>{t(`tasks.lane.${id}`)}</h2></div><em>{items.length}</em></header>
              <div className="financial-task-list">
                {items.length > 0 ? items.map((item) => <TaskCard item={item} key={`${item.kind}-${item.id}`} onOpen={onOpen} />) : <p>{t("tasks.laneEmpty")}</p>}
              </div>
            </section>
          ))}
        </div>
      )}
    </div>
  );
}
