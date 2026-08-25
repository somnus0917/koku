import { NAV_ITEMS } from "./nav";

// 底栏只保留四个高频账本入口；管理员页面仍可从移动端左上角菜单进入。
// 这避免管理员的第 5、6 项在固定高度的底栏中换行。
export const MOBILE_NAV_ITEMS = NAV_ITEMS.filter(
  (item) => item.id === "dashboard" || item.id === "accounts" || item.id === "transactions" || item.id === "insights"
);
