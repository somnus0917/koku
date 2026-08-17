//! 主题持久化：支持 浅色 / 深色 / 跟随系统 三态。
//! 偏好存 localStorage（key: koku-theme），跟随系统时监听
//! prefers-color-scheme 变化实时切换；最终主题通过
//! document.documentElement.dataset.theme 应用到全局（见 styles.css）。
import { useEffect, useState } from "react";

export type ThemePreference = "light" | "dark" | "system";
export type ResolvedTheme = "light" | "dark";

const THEME_KEY = "koku-theme";
const DARK_MEDIA = "(prefers-color-scheme: dark)";

function readPreference(): ThemePreference {
  try {
    const stored = window.localStorage.getItem(THEME_KEY);
    return stored === "light" || stored === "dark" || stored === "system"
      ? stored
      : "system";
  } catch {
    return "system";
  }
}

function systemResolved(): ResolvedTheme {
  return window.matchMedia(DARK_MEDIA).matches ? "dark" : "light";
}

function resolve(preference: ThemePreference): ResolvedTheme {
  return preference === "system" ? systemResolved() : preference;
}

/**
 * 返回 { theme, resolved, setTheme }：
 * - theme：用户偏好（light | dark | system）
 * - resolved：实际生效主题（light | dark）
 * - setTheme：切换偏好并持久化到 localStorage
 */
export function useTheme() {
  const [theme, setThemePreference] = useState<ThemePreference>(readPreference);
  const [resolved, setResolved] = useState<ResolvedTheme>(() =>
    resolve(readPreference())
  );

  // 应用最终主题到根元素。
  useEffect(() => {
    document.documentElement.dataset.theme = resolved;
  }, [resolved]);

  // 跟随系统：监听 prefers-color-scheme 变化实时切换。
  useEffect(() => {
    if (theme !== "system") return;
    const media = window.matchMedia(DARK_MEDIA);
    const apply = () => setResolved(media.matches ? "dark" : "light");
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [theme]);

  const setTheme = (next: ThemePreference) => {
    setThemePreference(next);
    if (next !== "system") setResolved(next);
    try {
      window.localStorage.setItem(THEME_KEY, next);
    } catch {
      // 隐私模式等场景下忽略写入失败。
    }
  };

  return { theme, resolved, setTheme };
}
