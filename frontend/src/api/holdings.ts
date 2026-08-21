//! 持仓 API：买卖股票与市价刷新。
import { request } from "./client";
import type { Holding, Transaction } from "../types";

export interface HoldingQuote {
  symbol: string;
  price: string;
  date: string;
  source: "stooq" | "yahoo_finance";
  market: Holding["market"];
}

export function buyStock(input: {
  account_id: number;
  symbol: string;
  shares: string;
  price: string;
  fee?: string;
  occurred_at?: string;
  note?: string;
}): Promise<Transaction> {
  return request("/api/holdings/buy", {
    method: "POST",
    body: JSON.stringify(input)
  });
}
export function sellStock(input: {
  account_id: number;
  symbol: string;
  shares: string;
  price: string;
  fee?: string;
  occurred_at?: string;
  note?: string;
}): Promise<Transaction> {
  return request("/api/holdings/sell", {
    method: "POST",
    body: JSON.stringify(input)
  });
}
/** 按代码查询当前参考价；未覆盖的标的可继续手动输入价格。 */
export function getHoldingQuote(symbol: string): Promise<HoldingQuote> {
  return request(`/api/holdings/quote?symbol=${encodeURIComponent(symbol)}`);
}
export function setHoldingPrice(holdingId: number, price: string): Promise<Holding> {
  return request(`/api/holdings/${holdingId}/price`, {
    method: "PUT",
    body: JSON.stringify({ price })
  });
}
/** 刷新过期/缺失市价的全部持仓（只刷过期项），返回刷新统计与最新持仓。 */
export function refreshHoldings(): Promise<{
  refreshed: number;
  failed: { symbol: string; error: string }[];
  holdings: Holding[];
}> {
  return request("/api/holdings/refresh", { method: "POST" });
}
/** 强制刷新单只持仓的市价。 */
export function refreshHolding(id: number): Promise<Holding> {
  return request(`/api/holdings/${id}/refresh`, { method: "POST" });
}
