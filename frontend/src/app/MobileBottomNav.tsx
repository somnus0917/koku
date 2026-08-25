//! 移动端底部导航。
import { useTranslation } from "react-i18next";
import { Plus } from "lucide-react";
import type { View } from "./nav";
import { MOBILE_NAV_ITEMS } from "./mobileNavItems";

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
