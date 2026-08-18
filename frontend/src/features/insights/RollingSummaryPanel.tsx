//! 滚动平均面板：最近 N 个月收支曲线与 trailing window 均值（自加载）。
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { RefreshCcw } from "lucide-react";
import { EmptyState } from "../../components/EmptyState";
import { loadRollingSummary } from "../../api";
import type { RollingSummary } from "../../types";

/** 滚动平均面板：自加载最近 N 个月的收支曲线 + trailing window 均值虚线。 */
export function RollingSummaryPanel({ currency }: { currency: string }) {
  const [monthsInput, setMonthsInput] = useState("12");
  const [windowInput, setWindowInput] = useState("3");
  const [summary, setSummary] = useState<RollingSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const { t } = useTranslation();
  const monthsSize = Number(monthsInput);
  const windowSize = Number(windowInput);
  const monthsValid = Number.isInteger(monthsSize) && monthsSize >= 1 && monthsSize <= 120;
  const windowValid = Number.isInteger(windowSize) && windowSize >= 1 && windowSize <= 120;
  const valid = monthsValid && windowValid;
  // 平均窗口不能超过趋势月数（后端上限约束），超限时按 months 截断。
  const effectiveWindow = valid ? Math.min(windowSize, monthsSize) : 0;
  useEffect(() => {
    if (!valid) return;
    let cancelled = false;
    setError(null);
    loadRollingSummary(monthsSize, effectiveWindow, currency)
      .then((data) => {
        if (!cancelled) {
          setSummary(data);
          setError(null);
        }
      })
      .catch((reason) => {
        if (!cancelled) setError(reason instanceof Error ? reason.message : t("insights.rolling.loadFailed"));
      });
    return () => {
      cancelled = true;
    };
  }, [monthsSize, effectiveWindow, currency, valid, attempt, t]);
  const windowClamped = windowValid && windowSize > monthsSize;
  return (
    <section className="panel trend-panel">
      <div className="section-heading compact-heading">
        <div><span>ROLLING AVG</span><h2>{t("insights.rolling.title")}</h2></div>
        <div className="rolling-controls">
          <label><span>{t("insights.rolling.months")}</span><input type="number" min={1} max={120} value={monthsInput} onChange={(e) => setMonthsInput(e.target.value)} aria-label={t("insights.rolling.monthsAria")} /></label>
          <label><span>{t("insights.rolling.window")}</span><input type="number" min={1} max={120} value={windowInput} onChange={(e) => setWindowInput(e.target.value)} aria-label={t("insights.rolling.windowAria")} /></label>
        </div>
      </div>
      {!valid && <div className="trend-note">{t("insights.rolling.invalid")}</div>}
      {windowClamped && <div className="trend-note">{t("insights.rolling.clamped", { months: monthsSize })}</div>}
      {error && (
        <div className="trend-note panel-error">
          <span>{t("insights.rolling.error")}{error}</span>
          <button type="button" className="text-button" onClick={() => setAttempt((value) => value + 1)}><RefreshCcw size={13} /> {t("common.retry")}</button>
        </div>
      )}
      {valid && !error && !summary && <div className="trend-note">{t("common.loading")}</div>}
      {valid && summary && (
        <>
          <RollingChart summary={summary} />
          <div className="trend-legend rolling-legend">
            <span className="legend-income"><i />{t("common.income")}</span>
            <span className="legend-expense"><i />{t("common.expense")}</span>
            <span className="legend-income avg"><i />{t("insights.rolling.incomeAvg")}</span>
            <span className="legend-expense avg"><i />{t("insights.rolling.expenseAvg")}</span>
          </div>
          <p className="rolling-hint">{t("insights.rolling.hint", { window: summary.window, currency: summary.currency })}</p>
        </>
      )}
    </section>
  );
}
/** 滚动平均曲线：收入/支出实线 + 各自 trailing avg 虚线。 */
function RollingChart({ summary }: { summary: RollingSummary }) {
  const points = summary.points;
  const { t } = useTranslation();
  if (points.length === 0) {
    return <EmptyState title={t("insights.rolling.emptyTitle")} detail={t("insights.rolling.emptyDetail")} />;
  }
  const width = 720;
  const height = 250;
  const padL = 54;
  const padR = 16;
  const padT = 18;
  const padB = 34;
  const incomes = points.map((point) => Number(point.income));
  const expenses = points.map((point) => Number(point.expense));
  const incomeAvgs = points.map((point) => Number(point.income_avg));
  const expenseAvgs = points.map((point) => Number(point.expense_avg));
  const max = Math.max(1, ...incomes, ...expenses, ...incomeAvgs, ...expenseAvgs);
  const innerW = width - padL - padR;
  const innerH = height - padT - padB;
  const x = (index: number) =>
    padL + (points.length === 1 ? innerW / 2 : (index / (points.length - 1)) * innerW);
  const y = (value: number) => padT + ((max - value) / max) * innerH;
  const path = (values: number[]) =>
    values.map((value, index) => `${index ? "L" : "M"}${x(index).toFixed(1)},${y(value).toFixed(1)}`).join(" ");
  const gridLines = [0, 0.25, 0.5, 0.75, 1].map((fraction) => padT + fraction * innerH);
  const labelEvery = Math.max(1, Math.ceil(points.length / 12));
  return (
    <div className="monthly-trend-chart">
      <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label={t("insights.rolling.chartAria")}>
        <title>{t("insights.rolling.chartTitle", { months: summary.months, window: summary.window, currency: summary.currency })}</title>
        {gridLines.map((gy) => (
          <line key={gy} x1={padL} x2={width - padR} y1={gy} y2={gy} className="grid-line" />
        ))}
        <path d={path(expenses)} className="trend-series expense" />
        <path d={path(incomes)} className="trend-series income" />
        <path d={path(expenseAvgs)} className="trend-series expense avg" />
        <path d={path(incomeAvgs)} className="trend-series income avg" />
        {points.map((point, index) => (
          <text
            key={`label-${point.year}-${point.month}`}
            x={x(index)}
            y={height - 10}
            textAnchor="middle"
            className="chart-axis-label"
          >
            {index % labelEvery === 0 ? (point.month === 1 ? t("insights.yearLabel", { year: point.year }) : t("insights.monthLabel", { month: point.month })) : ""}
          </text>
        ))}
      </svg>
    </div>
  );
}
