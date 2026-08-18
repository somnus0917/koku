//! 预算面板：按分类设置/清除月度预算。
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, MoreHorizontal, Trash2, X } from "lucide-react";
import { CategoryAvatar } from "../../components/avatar";
import { EmptyState } from "../../components/EmptyState";
import { categoryVisual, formatMoney } from "../../lib";
import type { Budget, Category, MonthlySummary } from "../../types";

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
