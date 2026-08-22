//! 应用导航定义：视图 id 与图标。
import {
  ChartNoAxesCombined,
  ListTodo,
  LayoutDashboard,
  ReceiptText,
  Settings,
  Users,
  WalletCards,
  type LucideIcon
} from "lucide-react";

export type View = "dashboard" | "tasks" | "accounts" | "transactions" | "insights" | "users" | "system";

export const NAV_ITEMS: Array<{ id: View; icon: LucideIcon }> = [
  { id: "dashboard", icon: LayoutDashboard },
  { id: "tasks", icon: ListTodo },
  { id: "accounts", icon: WalletCards },
  { id: "transactions", icon: ReceiptText },
  { id: "insights", icon: ChartNoAxesCombined },
  { id: "users", icon: Users },
  { id: "system", icon: Settings }
];
