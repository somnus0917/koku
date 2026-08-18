//! 月度收支趋势面板（自加载）。
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { EmptyState } from "../../components/EmptyState";
import { loadTrend } from "../../api";
import type { MonthlyTrendPoint } from "../../types";

export function MonthlyTrendPanel({ currency }: { currency: string }) {
  const [points, setPoints] = useState<MonthlyTrendPoint[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  useEffect(() => {
    let cancelled = false;
    loadTrend(12, currency)
      .then((data) => {
        if (!cancelled) {
          setPoints(data);
          setError(null);
        }
      })
      .catch((reason) => {
        if (!cancelled) setError(reason instanceof Error ? reason.message : t("insights.trendLoadFailed"));
      });
    return () => {
      cancelled = true;
    };
  }, [currency, t]);
  return (
    <section className="panel trend-panel">
      <div className="section-heading compact-heading">
        <div><span>TREND</span><h2>{t("insights.trendTitle")}</h2></div>
      </div>
      {error && <div className="trend-note">{t("insights.trendError")}{error}</div>}
      {!error && !points && <div className="trend-note">{t("common.loading")}</div>}
      {points && <MonthlyTrendChart points={points} currency={currency} />}
    </section>
  );
}
export function MonthlyTrendChart({ points, currency }: { points: MonthlyTrendPoint[]; currency: string }) {
  const { t } = useTranslation();
  if (points.length === 0) {
    return <EmptyState title={t("insights.trendEmptyTitle")} detail={t("insights.trendEmptyDetail")} />;
  }
  const width = 720;
  const height = 250;
  const padL = 54;
  const padR = 16;
  const padT = 18;
  const padB = 34;
  const incomes = points.map((point) => Number(point.total_income));
  const expenses = points.map((point) => Number(point.total_expense));
  const nets = points.map((point) => Number(point.net));
  const max = Math.max(1, ...incomes, ...expenses, ...nets);
  const min = Math.min(0, ...nets);
  const range = Math.max(1, max - min);
  const innerW = width - padL - padR;
  const innerH = height - padT - padB;
  const x = (index: number) =>
    padL + (points.length === 1 ? innerW / 2 : (index / (points.length - 1)) * innerW);
  const y = (value: number) => padT + ((max - value) / range) * innerH;
  const path = (values: number[]) =>
    values.map((value, index) => `${index ? "L" : "M"}${x(index).toFixed(1)},${y(value).toFixed(1)}`).join(" ");
  const gridLines = [0, 0.25, 0.5, 0.75, 1].map((fraction) => padT + fraction * innerH);
  return (
    <div className="monthly-trend-chart">
      <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label={t("insights.trendChartAria")}>
        <title>{t("insights.trendChartTitle", { currency })}</title>
        {gridLines.map((gy) => (
          <line key={gy} x1={padL} x2={width - padR} y1={gy} y2={gy} className="grid-line" />
        ))}
        <line x1={padL} x2={width - padR} y1={y(0)} y2={y(0)} className="trend-zero-line" />
        <path d={path(expenses)} className="trend-series expense" />
        <path d={path(incomes)} className="trend-series income" />
        <path d={path(nets)} className="trend-series net" />
        {points.map((point, index) => (
          <circle
            key={point.year * 100 + point.month}
            cx={x(index)}
            cy={y(Number(point.net))}
            r="3.2"
            className="trend-point"
          />
        ))}
        {points.map((point, index) => (
          <text
            key={`label-${point.year}-${point.month}`}
            x={x(index)}
            y={height - 10}
            textAnchor="middle"
            className="chart-axis-label"
          >
            {point.month === 1 ? t("insights.yearLabel", { year: point.year }) : t("insights.monthLabel", { month: point.month })}
          </text>
        ))}
      </svg>
      <div className="trend-legend">
        <span className="legend-income"><i />{t("common.income")}</span>
        <span className="legend-expense"><i />{t("common.expense")}</span>
        <span className="legend-net"><i />{t("common.net")}</span>
      </div>
    </div>
  );
}
