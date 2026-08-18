//! 跨币种汇率提示：参考汇率文本、自动拉取 /api/rates 的 hook 与展示行。
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { RefreshCcw } from "lucide-react";
import { rateHint } from "../api";
import type { RateQuote } from "../types";

/** 参考汇率文本：如 1 USD ≈ 7.1445 CNY（2026-08-14）。 */
export function formatRate(rate: string) {
  return Number(rate).toFixed(4).replace(/\.?0+$/, "");
}

/** 跨币种汇率提示：自动拉取 /api/rates（服务端带缓存与多源回退），失败时可手动重试。 */
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

export function RateHintLine({
  from,
  to,
  status,
  hint,
  onRefresh
}: {
  from: string;
  to: string;
  status: "idle" | "loading" | "ok" | "error";
  hint: RateQuote | null;
  onRefresh?: () => void;
}) {
  const { t } = useTranslation();
  if (status === "loading") {
    return <p className="fx-hint">{t("modals.rate.loading")}</p>;
  }
  if (status === "ok" && hint) {
    return (
      <p className="fx-hint">
        {t("modals.rate.hint", {
          from,
          rate: formatRate(hint.rate),
          to,
          meta: t("modals.rate.meta", { date: hint.date, source: hint.source, stale: hint.stale ? t("modals.rate.stale") : "" })
        })}
        {onRefresh && (
          <button type="button" className="fx-hint-refresh" onClick={onRefresh} title={t("modals.rate.refreshAria")} aria-label={t("modals.rate.refreshAria")}>
            <RefreshCcw size={11} />
          </button>
        )}
      </p>
    );
  }
  if (status === "error") {
    return (
      <p className="fx-hint error">
        {t("modals.rate.error")}
        {onRefresh && (
          <button type="button" className="fx-hint-refresh" onClick={onRefresh}>
            {t("modals.rate.retry")}
          </button>
        )}
      </p>
    );
  }
  return null;
}
