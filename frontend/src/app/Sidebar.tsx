//! 侧边栏：品牌、主导航与个人操作区。
import { useTranslation } from "react-i18next";
import { KeyRound, LogOut, MoreHorizontal, ShieldCheck, Sparkles, X } from "lucide-react";
import { NAV_ITEMS, type View } from "./nav";
import type { UserRole } from "../types";

export function Sidebar({
  username,
  role,
  activeView,
  onNavigate,
  onOpenCategories,
  onOpenPassword,
  onOpenTotp,
  onOpenLearningSettings,
  onLogout,
  mobileNavOpen,
  onCloseMobileNav
}: {
  username: string;
  role: UserRole;
  activeView: View;
  onNavigate: (view: View) => void;
  onOpenCategories: () => void;
  onOpenPassword: () => void;
  onOpenTotp: () => void;
  onOpenLearningSettings: () => void;
  onLogout: () => void;
  mobileNavOpen: boolean;
  onCloseMobileNav: () => void;
}) {
  const { t } = useTranslation();
  return (
    <aside className={`sidebar ${mobileNavOpen ? "sidebar-open" : ""}`}>
      <div className="brand">
        <div className="brand-mark" aria-hidden="true">
          <span />
          <span />
        </div>
        <div>
          <strong>Koku</strong>
          <small>PRIVATE LEDGER</small>
        </div>
        <button className="mobile-close" onClick={onCloseMobileNav} aria-label={t("nav.closeMenu")}>
          <X size={20} />
        </button>
      </div>

      <nav className="primary-nav" aria-label={t("nav.main")}>
        {NAV_ITEMS.filter((item) => role === "admin" || (item.id !== "users" && item.id !== "system")).map(({ id, icon: Icon }) => (
          <button
            className={activeView === id ? "active" : ""}
            key={id}
            onClick={() => onNavigate(id)}
          >
            <Icon size={19} strokeWidth={1.8} />
            <span>{t(`nav.${id}`)}</span>
          </button>
        ))}
      </nav>

      <div className="sidebar-spacer" />
      <div className="profile-actions">
        <button className="profile-chip" onClick={onOpenCategories}>
          <span className="avatar">K</span>
          <span title={username}>
            <strong>{username}</strong>
          </span>
          <MoreHorizontal size={18} />
        </button>
        <button className="password-button" onClick={onOpenPassword} aria-label={t("common.changePassword")} title={t("common.changePassword")}><KeyRound size={17} /></button>
        <button className="password-button" onClick={onOpenTotp} aria-label={t("totp.title")} title={t("totp.title")}><ShieldCheck size={17} /></button>
        <button className="password-button" onClick={onOpenLearningSettings} aria-label={t("settings.learningTitle")} title={t("settings.learningTitle")}><Sparkles size={17} /></button>
        <button className="logout-button" onClick={onLogout} aria-label={t("common.logout")} title={t("common.logout")}><LogOut size={17} /></button>
      </div>
    </aside>
  );
}
