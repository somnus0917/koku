//! i18n 初始化：zh（默认，与原有硬编码文案逐字一致）/ en 双语言。
//! 语言偏好存 localStorage（key: koku-lang），切换通过 changeLanguage 便捷封装。

import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { zh } from "./locales/zh";
import { en } from "./locales/en";

const resources = { zh, en };

void i18n.use(initReactI18next).init({
  resources,
  lng: readLanguage(),
  fallbackLng: "zh",
  interpolation: {
    escapeValue: false
  },
  react: {
    useSuspense: false
  }
});


function readLanguage(): string {
  try {
    const stored = window.localStorage.getItem("koku-lang");
    return stored === "zh" || stored === "en" ? stored : "zh";
  } catch {
    return "zh";
  }
}


/** 切换语言：写入 localStorage 并让 i18next 生效（react-i18next 自动触发重渲染）。 */
export function changeLanguage(lng: "zh" | "en"): Promise<unknown> {
  try {
    window.localStorage.setItem("koku-lang", lng);
  } catch {
    // 隐私模式等场景下忽略写入失败。
  }
  return i18n.changeLanguage(lng);
}

/** 当前语言对应的 Intl locale（en → en-US，其余 → zh-CN）。 */
export function uiLocale(): string {
  return i18n.language?.toLowerCase().startsWith("en") ? "en-US" : "zh-CN";
}


export default i18n;
