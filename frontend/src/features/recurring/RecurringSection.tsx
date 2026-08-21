//! 周期交易区块（账户页）：规则列表与删除。
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDownLeft, ArrowUpRight, Pause, Pencil, Play, Plus, Trash2 } from "lucide-react";
import { getRecurringPreview } from "../../api";
import { EmptyState } from "../../components/EmptyState";
import { formatDate, formatMoney } from "../../lib";
import type { Account, Category, RecurringRule } from "../../types";

export function RecurringSection({
  rules,
  accounts,
  categories,
  onCreate,
  onDelete,
  onEdit,
  onTogglePaused
}: {
  rules: RecurringRule[];
  accounts: Account[];
  categories: Category[];
  onCreate: () => void;
  onDelete: (id: number) => void;
  onEdit: (rule: RecurringRule) => void;
  onTogglePaused: (rule: RecurringRule) => void;
}) {
  const accountMap = useMemo(() => new Map(accounts.map((account) => [account.id, account])), [accounts]);
  const categoryMap = useMemo(() => new Map(categories.map((category) => [category.id, category])), [categories]);
  const { t } = useTranslation();
  const [previews, setPreviews] = useState<Record<number, string[]>>({});
  useEffect(() => {
    let cancelled = false;
    Promise.all(rules.map(async (rule) => [rule.id, (await getRecurringPreview(rule.id)).map((item) => item.due_at)] as const))
      .then((values) => { if (!cancelled) setPreviews(Object.fromEntries(values)); })
      .catch(() => undefined);
    return () => { cancelled = true; };
  }, [rules]);
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
                  {rule.paused_at ? ` · ${t("accounts.recurring.paused")}` : previews[rule.id]?.length ? ` · ${previews[rule.id].map(formatDate).join(" / ")}` : ""}
                </span>
              </div>
              <strong className={isExpense ? "expense-text" : "income-text"}>
                {isExpense ? "−" : "+"}{formatMoney(rule.amount, account?.currency ?? "CNY")}
              </strong>
              <button className="row-action" onClick={() => onEdit(rule)} title={t("accounts.recurring.edit")} aria-label={t("accounts.recurring.edit")}><Pencil size={16} /></button>
              <button className="row-action" onClick={() => onTogglePaused(rule)} title={t(rule.paused_at ? "accounts.recurring.resume" : "accounts.recurring.pause")} aria-label={t(rule.paused_at ? "accounts.recurring.resume" : "accounts.recurring.pause")}>{rule.paused_at ? <Play size={16} /> : <Pause size={16} />}</button>
              <button className="row-action" onClick={() => onDelete(rule.id)} title={t("accounts.recurring.delete")} aria-label={t("accounts.recurring.delete")}><Trash2 size={16} /></button>
            </article>
          );
        })}
        {rules.length === 0 && <EmptyState title={t("accounts.recurring.emptyTitle")} detail={t("accounts.recurring.emptyDetail")} />}
      </div>
    </section>
  );
}
