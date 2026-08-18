//! 移动端底部导航。
import { useTranslation } from "react-i18next";
import { Plus } from "lucide-react";
import { NAV_ITEMS, type View } from "./nav";
import type { UserRole } from "../types";

export function MobileBottomNav({
  role,
  activeView,
  onNavigate,
  onQuickAdd
}: {
  role: UserRole;
  activeView: View;
  onNavigate: (view: View) => void;
  onQuickAdd: () => void;
}) {
  const { t } = useTranslation();
  return (
    <nav className="mobile-bottom-nav" aria-label={t("nav.mobile")}>
      {NAV_ITEMS.filter((item) => role === "admin" || (item.id !== "users" && item.id !== "system")).map(({ id, icon: Icon }) => (
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
