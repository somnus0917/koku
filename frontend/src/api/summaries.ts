//! 汇总 API：首屏汇总数据与月度/趋势/标签/年度/滚动汇总。
import { request } from "./client";
import type {
  Account,
  AppData,
  BalanceSummary,
  Budget,
  CashFlowSummary,
  Category,
  Deposit,
  Holding,
  Loan,
  MonthlySummary,
  MonthlyTrendPoint,
  RecurringRule,
  RollingSummary,
  Tag,
  TagSummary,
  YearlySummary
} from "../types";

/** 汇总侧数据（除交易流水外的所有首屏数据）。 */
export type SummaryData = Omit<AppData, "transactions">;

export async function loadSummaryData(
  year: number,
  month: number,
  currency: string
): Promise<SummaryData> {
  const query = new URLSearchParams({
    year: String(year),
    month: String(month),
    currency
  });
  const currencyQuery = new URLSearchParams({ currency });
  const budgetQuery = new URLSearchParams({ year: String(year), month: String(month) });
  const [accounts, categories, budgets, monthly, cashFlow, balance, loans, recurring, tags, holdings, deposits] = await Promise.all([
    request<Account[]>("/api/accounts"),
    request<Category[]>("/api/categories"),
    request<Budget[]>(`/api/budgets?${budgetQuery}`),
    request<MonthlySummary>(`/api/summary/monthly?${query}`),
    request<CashFlowSummary>(`/api/summary/cash-flow?${query}`),
    request<BalanceSummary>(`/api/summary/balance?${currencyQuery}`),
    request<Loan[]>("/api/loans"),
    request<RecurringRule[]>("/api/recurring"),
    request<Tag[]>("/api/tags"),
    request<Holding[]>("/api/holdings"),
    request<Deposit[]>("/api/deposits")
  ]);
  return { accounts, categories, budgets, monthly, cashFlow, balance, loans, recurring, tags, holdings, deposits };
}
/** 查询最近 `months` 个月的收支趋势（收入/支出/结余逐月折算到显示币种）。 */
export function loadTrend(months: number, currency: string): Promise<MonthlyTrendPoint[]> {
  const query = new URLSearchParams({ months: String(months), currency });
  return request<MonthlyTrendPoint[]>(`/api/summary/trend?${query.toString()}`);
}
/**
 * 标签汇总：同时带有全部指定标签的收支流水合计。
 * 传 `year`/`month` 统计该自然月；不传则统计全部历史。
 */
export function loadTagSummary(
  tags: string[],
  currency: string,
  year?: number,
  month?: number
): Promise<TagSummary> {
  const query = new URLSearchParams({ tags: tags.join(","), currency });
  if (year !== undefined && month !== undefined) {
    query.set("year", String(year));
    query.set("month", String(month));
  }
  return request<TagSummary>(`/api/summary/by-tag?${query.toString()}`);
}
/** 年度汇总：某自然年的逐月收支 + 全年合计 + 分类明细。 */
export function loadYearlySummary(year: number, currency: string): Promise<YearlySummary> {
  const query = new URLSearchParams({ year: String(year), currency });
  return request<YearlySummary>(`/api/summary/yearly?${query.toString()}`);
}
/** 滚动平均：最近 `months` 个月的收支趋势 + trailing window 均值。 */
export function loadRollingSummary(
  months: number,
  window: number,
  currency: string
): Promise<RollingSummary> {
  const query = new URLSearchParams({
    months: String(months),
    window: String(window),
    currency
  });
  return request<RollingSummary>(`/api/summary/rolling?${query.toString()}`);
}
