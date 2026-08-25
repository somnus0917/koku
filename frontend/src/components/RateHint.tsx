//! 跨币种汇率提示展示行。
import { useTranslation } from "react-i18next";
import { RefreshCcw } from "lucide-react";
import type { RateQuote } from "../types";
import { formatRate } from "./rateHintState";

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
