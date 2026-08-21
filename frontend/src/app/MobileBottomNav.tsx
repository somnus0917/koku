//! 移动端底部导航。
import { useTranslation } from "react-i18next";
import { Plus } from "lucide-react";
import { NAV_ITEMS, type View } from "./nav";

// 底栏只保留四个高频账本入口；管理员页面仍可从移动端左上角菜单进入。
// 这避免管理员的第 5、6 项在固定高度的底栏中换行。
export const MOBILE_NAV_ITEMS = NAV_ITEMS.filter(
  (item) => item.id === "dashboard" || item.id === "accounts" || item.id === "transactions" || item.id === "insights"
);

export function MobileBottomNav({
  activeView,
  onNavigate,
  onQuickAdd
}: {
  activeView: View;
  onNavigate: (view: View) => void;
  onQuickAdd: () => void;
}) {
  const { t } = useTranslation();
  return (
    <nav className="mobile-bottom-nav" aria-label={t("nav.mobile")}>
      {MOBILE_NAV_ITEMS.map(({ id, icon: Icon }) => (
        <button key={id} className={activeView === id ? "active" : ""} onClick={() => onNavigate(id)}>
          <Icon size={20} />
          <span>{t(`nav.${id}`)}</span>
        </button>
      ))}
      <button className="mobile-add" onClick={onQuickAdd} aria-label={t("common.quickAdd")}>
        <Plus size={23} />
      </button>
    </nav>
  );
}
