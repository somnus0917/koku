//! 汇率提示 API。
import { request } from "./client";
import type { RateQuote } from "../types";

/** 汇率提示：1 from = rate to，服务端带本地缓存，数据源不可达时可能返回 stale 快照。 */
export function rateHint(from: string, to: string): Promise<RateQuote> {
  const query = new URLSearchParams({ from, to });
  return request(`/api/rates?${query.toString()}`);
}
