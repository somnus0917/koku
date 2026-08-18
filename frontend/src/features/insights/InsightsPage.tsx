//! 分析页：月度/年度/滚动趋势、现金流桑基与预算面板。
import type { CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { ChartNoAxesCombined } from "lucide-react";
import { PageTitle } from "../../components/PageTitle";
import { SummaryCard } from "../../components/SummaryCard";
import { CategoryAvatar } from "../../components/avatar";
import { buildDonutGradient, formatMoney, healthScore } from "../../lib";
import { MonthlyTrendPanel } from "./MonthlyTrendPanel";
import { YearlySummaryPanel } from "./YearlySummaryPanel";
import { RollingSummaryPanel } from "./RollingSummaryPanel";
import { CashFlowSankey } from "./CashFlowSankey";
import { CategoryBars } from "./CategoryBars";
import { BudgetPanel } from "../budgets/BudgetPanel";
import type { Budget, CashFlowSummary, Category, MonthlySummary } from "../../types";

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
