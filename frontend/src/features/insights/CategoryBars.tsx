//! 分类支出条形图。
import { useTranslation } from "react-i18next";
import { CategoryAvatar } from "../../components/avatar";
import { EmptyState } from "../../components/EmptyState";
import { categoryVisual, formatMoney } from "../../lib";
import type { MonthlySummary } from "../../types";

export function CategoryBars({ summary, detailed = false }: { summary: MonthlySummary; detailed?: boolean }) {
  const { t } = useTranslation();
  if (!summary.expenses_by_category.length) return <EmptyState title={t("insights.categoryBars.emptyTitle")} detail={t("insights.categoryBars.emptyDetail")} />;
  return (
    <div className={`category-bars ${detailed ? "detailed" : ""}`}>
      {summary.expenses_by_category.slice(0, detailed ? 8 : 4).map((item) => (
        <div className="category-bar" key={item.category_id}>
          <div><span><CategoryAvatar name={item.category_name} size="small" />{item.category_name}</span><strong>{formatMoney(item.amount, summary.currency)}</strong></div>
          <div className="bar-track"><i style={{ width: `${item.percentage}%`, background: categoryVisual(item.category_name).color }} /></div>
          {detailed && <small>{t("insights.categoryBars.percentLabel", { percent: item.percentage })}</small>}
        </div>
      ))}
    </div>
  );
}
