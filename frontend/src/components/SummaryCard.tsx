//! 统计卡片：标签 + 数值。
import { formatMoney } from "../lib";

export function SummaryCard({ label, value, currency, tone }: { label: string; value: string; currency: string; tone: string }) {
  return (
    <article className={`summary-card ${tone}`}>
      <span>{label}</span>
      <strong>{formatMoney(value, currency)}</strong>
    </article>
  );
}
