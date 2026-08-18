//! 分类 API：新建与删除。
import { request } from "./client";
import type { Category, CategoryKind } from "../types";

export function createCategory(input: {
  name: string;
  kind: CategoryKind;
}): Promise<Category> {
  return request("/api/categories", {
    method: "POST",
    body: JSON.stringify(input)
  });
}
export function deleteCategory(id: number): Promise<Category> {
  return request(`/api/categories/${id}`, { method: "DELETE" });
}
