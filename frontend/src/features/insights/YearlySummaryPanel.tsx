//! 年度汇总面板：逐月柱状图与分类明细（自加载）。
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronLeft, ChevronRight, RefreshCcw } from "lucide-react";
import { SummaryCard } from "../../components/SummaryCard";
import { CategoryAvatar } from "../../components/avatar";
import { loadYearlySummary } from "../../api";
import { formatMoney } from "../../lib";
import type { CashFlowItem, YearlySummary } from "../../types";

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
