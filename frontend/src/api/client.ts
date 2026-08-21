//! API 客户端基础：base URL、请求封装、错误类型与 401 全局处理。
import i18n from "../i18n";

export const API_BASE = import.meta.env.VITE_API_BASE_URL ?? "";
export interface Envelope<T> {
  data: T;
}
export class ApiError extends Error {
  constructor(message: string, readonly status: number) {
    super(message);
    this.name = "ApiError";
  }
}
export async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, {
    ...init,
    credentials: "same-origin",
    headers: {
      "Content-Type": "application/json",
      ...init?.headers
    }
  });
  if (response.status === 204) return undefined as T;
  const payload = (await response.json().catch(() => ({}))) as Partial<Envelope<T>> & {
    error?: string;
  };
  if (!response.ok) {
    if (response.status === 401 && path !== "/api/auth/login") {
      window.dispatchEvent(new Event("koku:unauthorized"));
    }
    throw new ApiError(payload.error ?? i18n.t("api.requestFailed", { status: response.status }), response.status);
  }
  if (payload.data === undefined) {
    throw new Error(i18n.t("api.invalidData"));
  }
  return payload.data;
}
