//! 持仓区块（账户页）：市值、买卖与市价刷新。
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check, Plus, RefreshCcw, TrendingUp, X } from "lucide-react";
import { EmptyState } from "../../components/EmptyState";
import { convertedMoney } from "../../components/accountDisplay";
import { formatDate, formatMoney } from "../../lib";
import type { Account, Holding } from "../../types";

export function HoldingSection({
  holdings,
  accounts,
  display,
  rates,
  onBuy,
  onSell,
  onSetPrice,
  onRefreshHoldings,
  onRefreshHolding
}: {
  holdings: Holding[];
  accounts: Account[];
  display: string;
  rates: Record<string, number>;
  onBuy: (symbol?: string) => void;
  onSell: (symbol: string) => void;
  onSetPrice: (holdingId: number, price: string) => void;
  /** 刷新过期/缺失市价。 */
  onRefreshHoldings: () => void;
  /** 强制刷新单只持仓市价（可选）。 */
  onRefreshHolding?: (holdingId: number) => void;
}) {
  const accountMap = useMemo(() => new Map(accounts.map((account) => [account.id, account])), [accounts]);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [draft, setDraft] = useState("");
  const { t } = useTranslation();
  // 挂载后懒拉取：只要存在从未刷新或超过 24 小时未刷新的持仓，就在后台刷新一次市价。
  const didAutoRefresh = useRef(false);
  useEffect(() => {
    if (didAutoRefresh.current) return;
    const stale = holdings.some((holding) => {
      if (!holding.updated_at) return true;
      return Date.now() - Date.parse(holding.updated_at) > 24 * 3600 * 1000;
    });
    if (!stale) return;
    didAutoRefresh.current = true;
    Promise.resolve()
      .then(() => onRefreshHoldings())
      .catch(() => undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [holdings]);
  return (
    <section className="section-block account-group">
      <div className="section-heading compact-heading">
        <div><span>HOLDINGS</span><h2>{t("holdings.title")}</h2></div>
        <div className="holding-actions">
          <button className="text-button" onClick={() => void onRefreshHoldings()} title={t("holdings.refreshStaleTitle")} aria-label={t("holdings.refreshAria")}><RefreshCcw size={14} /> {t("holdings.refresh")}</button>
          <button className="text-button" onClick={() => onBuy()}><Plus size={16} /> {t("holdings.buy")}</button>
        </div>
      </div>
      <div className="account-grid">
        {holdings.map((holding) => {
          const account = accountMap.get(holding.account_id);
          const currency = account?.currency ?? "CNY";
          const shares = Number(holding.shares);
          const lastPrice = holding.last_price !== null ? Number(holding.last_price) : null;
          const marketValue = shares * (lastPrice ?? Number(holding.average_cost));
          const shown = convertedMoney(String(marketValue), currency, display, rates)
            ?? { amount: String(marketValue), currency };
          const editing = editingId === holding.id;
          return (
            <article className="account-detail-card" key={holding.id}>
              <span className="large-account-icon tone-3"><TrendingUp size={23} /></span>
              <div className="account-detail-copy">
                <h3>{holding.symbol}</h3>
                <span>
                  {t("holdings.meta", { shares, cost: formatMoney(holding.average_cost, currency) })}
                  {lastPrice !== null ? t("holdings.metaPrice", { price: formatMoney(holding.last_price!, currency) }) : t("holdings.noPrice")}
                  {holding.updated_at ? t("holdings.updatedAt", { date: formatDate(holding.updated_at) }) : ""}
                </span>
              </div>
              <strong>{formatMoney(shown.amount, shown.currency)}</strong>
              {holding.unrealized_gain !== null && (
                <span className={Number(holding.unrealized_gain) >= 0 ? "holding-gain positive" : "holding-gain negative"}>
                  {Number(holding.unrealized_gain) >= 0 ? "+" : ""}{formatMoney(holding.unrealized_gain, currency)}
                  {holding.unrealized_return_percent !== null ? ` (${Number(holding.unrealized_return_percent) >= 0 ? "+" : ""}${holding.unrealized_return_percent}%)` : ""}
                </span>
              )}
              <div className="account-card-actions">
                {editing ? (
                  <>
                    <input className="inline-number" type="number" min="0" step="0.01" value={draft} onChange={(event) => setDraft(event.target.value)} placeholder={t("holdings.pricePlaceholder")} autoFocus />
                    <button className="row-action" onClick={() => { if (draft.trim()) onSetPrice(holding.id, draft.trim()); setEditingId(null); }} title={t("holdings.savePrice")} aria-label={t("holdings.savePrice")}><Check size={16} /></button>
                    <button className="row-action" onClick={() => setEditingId(null)} title={t("common.cancel")} aria-label={t("common.cancel")}><X size={16} /></button>
                  </>
                ) : (
                  <>
                    <button className="row-action" onClick={() => { setDraft(holding.last_price ?? ""); setEditingId(holding.id); }} title={t("holdings.updatePrice")} aria-label={t("holdings.updatePrice")}><RefreshCcw size={16} /></button>
                    {onRefreshHolding && (
                      <button className="text-button" onClick={() => void onRefreshHolding(holding.id)} title={t("holdings.forceRefresh")}>{t("holdings.refreshShort")}</button>
                    )}
                    <button className="text-button" onClick={() => onBuy(holding.symbol)}>{t("holdings.buyShort")}</button>
                    <button className="text-button" onClick={() => onSell(holding.symbol)}>{t("holdings.sellShort")}</button>
                  </>
                )}
              </div>
            </article>
          );
        })}
        {holdings.length === 0 && <EmptyState title={t("holdings.emptyTitle")} detail={t("holdings.emptyDetail")} />}
      </div>
    </section>
  );
}
