//! 纯函数与静态常量：与 React 组件解耦，便于单元测试。
import {
  BadgeDollarSign,
  BriefcaseBusiness,
  Building2,
  Car,
  ChartCandlestick,
  CircleEllipsis,
  Cloud,
  Dumbbell,
  Gamepad2,
  Gift,
  GraduationCap,
  Handshake,
  HeartPulse,
  House,
  Landmark,
  Laptop,
  PawPrint,
  Percent,
  Plane,
  ReceiptText,
  RotateCcw,
  Shield,
  ShoppingBag,
  Smartphone,
  Tags,
  Trophy,
  Users,
  Utensils,
  Zap,
  type LucideIcon
} from "lucide-react";
import type { Account, MonthlySummary, Transaction } from "./types";

export const CATEGORY_COLORS = ["#274e3f", "#dd8d5b", "#7e95c9", "#d2ad58", "#8f6faf", "#669b92"];

export const COMMON_CURRENCIES = ["CNY", "USD", "HKD", "EUR", "JPY", "GBP"];

export const CATEGORY_VISUALS: Record<string, { icon: LucideIcon; color: string }> = {
  工资: { icon: BriefcaseBusiness, color: "#2c8765" },
  奖金: { icon: Trophy, color: "#c08a2f" },
  副业: { icon: Laptop, color: "#5078a5" },
  投资收益: { icon: ChartCandlestick, color: "#338c78" },
  利息: { icon: Percent, color: "#7a8f45" },
  报销: { icon: ReceiptText, color: "#6f76a8" },
  礼金: { icon: Gift, color: "#b26783" },
  退款: { icon: RotateCcw, color: "#4c918b" },
  其他收入: { icon: BadgeDollarSign, color: "#668562" },
  餐饮: { icon: Utensils, color: "#d0784e" },
  交通: { icon: Car, color: "#5077a5" },
  购物: { icon: ShoppingBag, color: "#ad6687" },
  居家: { icon: House, color: "#a47748" },
  娱乐: { icon: Gamepad2, color: "#7766a9" },
  医疗保健: { icon: HeartPulse, color: "#c55f64" },
  教育: { icon: GraduationCap, color: "#527ea0" },
  旅行: { icon: Plane, color: "#438b92" },
  通讯: { icon: Smartphone, color: "#6576a5" },
  水电燃气: { icon: Zap, color: "#bd8c30" },
  住房: { icon: Building2, color: "#8b704f" },
  保险: { icon: Shield, color: "#527d70" },
  数字订阅: { icon: Cloud, color: "#657fa8" },
  运动健身: { icon: Dumbbell, color: "#4f8e67" },
  宠物: { icon: PawPrint, color: "#a77457" },
  人情往来: { icon: Handshake, color: "#9b6c83" },
  家庭: { icon: Users, color: "#a06d50" },
  税费: { icon: Landmark, color: "#7e7165" },
  其他支出: { icon: CircleEllipsis, color: "#777b75" }
};

/** 分类视觉：预设优先，自定义分类按名称哈希生成稳定样式。 */
export function categoryVisual(name: string): { icon: LucideIcon; color: string } {
  const preset = CATEGORY_VISUALS[name];
  if (preset) return preset;
  const hash = [...name].reduce((value, character) => value + (character.codePointAt(0) ?? 0), 0);
  return { icon: Tags, color: CATEGORY_COLORS[hash % CATEGORY_COLORS.length] };
}

/** 金额格式化：Intl 货币格式，支持紧凑展示（万/亿）。 */
export function formatMoney(value: string, currency: string, compact = false): string {
  const number = Number(value);
  if (!Number.isFinite(number)) return `${value} ${currency}`;
  return new Intl.NumberFormat("zh-CN", {
    style: "currency",
    currency,
    currencyDisplay: "narrowSymbol",
    minimumFractionDigits: compact ? 0 : 2,
    maximumFractionDigits: compact ? 1 : 2,
    notation: compact ? "compact" : "standard"
  }).format(number);
}

/** 常见币种 + 已有账户/交易币种，去重后按固定顺序返回。 */
export function availableCurrencies(accounts: Account[], transactions: Transaction[] = []): string[] {
  return [
    ...new Set([
      ...COMMON_CURRENCIES,
      ...accounts.map((account) => account.currency),
      ...transactions.map((transaction) => transaction.currency)
    ])
  ];
}

/** 收支健康度：结余占收入百分比，0–100。 */
export function healthScore(summary: MonthlySummary): number {
  const income = Number(summary.total_income);
  const expense = Number(summary.total_expense);
  if (income <= 0) return expense === 0 ? 100 : 0;
  return Math.max(0, Math.min(100, Math.round(((income - expense) / income) * 100)));
}

/** 环形图渐变：按支出分类占比生成颜色断点。 */
export function buildDonutGradient(summary: MonthlySummary): string {
  if (!summary.expenses_by_category.length) return "var(--border) 0 100%";
  let cursor = 0;
  return summary.expenses_by_category
    .map((item) => {
      const start = cursor;
      cursor += Number(item.percentage);
      return `${categoryVisual(item.category_name).color} ${start}% ${cursor}%`;
    })
    .join(", ");
}

/** 当前月份 YYYY-MM。 */
export function currentMonthValue(): string {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}`;
}

/** 本地时区的 datetime-local 值（YYYY-MM-DDTHH:mm）。 */
export function localDateTimeValue(): string {
  const now = new Date();
  const offset = now.getTimezoneOffset() * 60_000;
  return new Date(now.getTime() - offset).toISOString().slice(0, 16);
}

/** 简短日期时间（如 "8月15日 14:30"）。 */
export function formatDate(value: string): string {
  return new Intl.DateTimeFormat("zh-CN", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit"
  }).format(new Date(value));
}
