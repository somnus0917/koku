//! 分类 API：新建与删除。
import { request } from "./client";
import type { Category, CategoryKind } from "../types";

export function createCategory(input: {
  name: string;
  kind: CategoryKind;
  /** 用户自选图标（lucide 图标名）。 */
  icon?: string;
}): Promise<Category> {
  return request("/api/categories", {
    method: "POST",
    body: JSON.stringify(input)
  });
}
export function deleteCategory(id: number): Promise<Category> {
  return request(`/api/categories/${id}`, { method: "DELETE" });
}
