//! 跨币种汇率提示状态：格式化参考汇率并自动拉取 /api/rates。
import { useEffect, useState } from "react";
import { rateHint } from "../api";
import type { RateQuote } from "../types";

/** 参考汇率文本：如 1 USD ≈ 7.1445 CNY（2026-08-14）。 */
export function formatRate(rate: string) {
  return Number(rate).toFixed(4).replace(/\.?0+$/, "");
}

/** 自动拉取 /api/rates（服务端带缓存与多源回退），失败时可手动重试。 */
export function useRateHint(from: string | null, to: string | null) {
  const [hint, setHint] = useState<RateQuote | null>(null);
  const [status, setStatus] = useState<"idle" | "loading" | "ok" | "error">("idle");
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    if (!from || !to || from === to) {
      setHint(null);
      setStatus("idle");
      return;
    }
    let cancelled = false;
    setStatus("loading");
    rateHint(from, to)
      .then((quote) => {
        if (!cancelled) {
          setHint(quote);
          setStatus("ok");
        }
      })
      .catch(() => {
        if (!cancelled) {
          setHint(null);
          setStatus("error");
        }
      });
    return () => {
      cancelled = true;
    };
  }, [from, to, attempt]);
  const refresh = () => setAttempt((n) => n + 1);
  return { hint, status, refresh };
}
