//! 账户展示辅助：账户图标、币种折算与折算汇率 hook。
import { useEffect, useState } from "react";
import { Banknote, CreditCard, PiggyBank, TrendingUp, WalletCards, type LucideIcon } from "lucide-react";
import { rateHint } from "../api";
import type { Account } from "../types";

export function accountIcon(account: Account): LucideIcon {
  if (account.account_type === "savings") return PiggyBank;
  if (account.account_type === "stock") return TrendingUp;
  if (account.account_type === "credit") return CreditCard;
  if (account.name.includes("现金")) return Banknote;
  return WalletCards;
}
/** 把金额从原币种折算到显示币种：同币种或缺失汇率时返回 null（调用方回退原币显示）。 */
export function convertedMoney(
  value: string,
  from: string,
  display: string,
  rates: Record<string, number> | undefined
): { amount: string; currency: string } | null {
  if (from === display) return null;
  const factor = rates?.[from];
  if (factor == null) return null;
  return { amount: (Number(value) * factor).toFixed(2), currency: display };
}
/** 拉取一批币种到显示币种的折算汇率：currency → 1 unit = factor display。 */
export function useConversionRates(currencies: string[], display: string) {
  const [rates, setRates] = useState<Record<string, number>>({});
  const key = currencies.join("|");
  useEffect(() => {
    const needed = [...new Set(key ? key.split("|") : [])].filter((currency) => currency !== display);
    if (needed.length === 0) {
      setRates({});
      return;
    }
    let cancelled = false;
    Promise.all(
      needed.map(async (currency) => {
        try {
          const quote = await rateHint(currency, display);
          return [currency, Number(quote.rate)] as const;
        } catch {
          return null;
        }
      })
    ).then((pairs) => {
      if (!cancelled) {
        setRates(
          Object.fromEntries(
            pairs.filter((pair): pair is readonly [string, number] => pair != null)
          )
        );
      }
    });
    return () => {
      cancelled = true;
    };
  }, [key, display]);
  return rates;
}
