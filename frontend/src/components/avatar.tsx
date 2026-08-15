//! 共享小组件：分类头像。
import type { CSSProperties } from "react";
import { categoryVisual } from "../lib";

export function CategoryAvatar({
  name,
  size = "medium",
  className = ""
}: {
  name: string;
  size?: "tiny" | "small" | "medium";
  className?: string;
}) {
  const visual = categoryVisual(name);
  const Icon = visual.icon;
  const iconSize = size === "tiny" ? 11 : size === "small" ? 14 : 18;
  return (
    <span
      className={`category-avatar ${size} ${className}`}
      style={{ "--category-color": visual.color } as CSSProperties}
      aria-hidden="true"
    >
      <Icon size={iconSize} strokeWidth={1.9} />
    </span>
  );
}
