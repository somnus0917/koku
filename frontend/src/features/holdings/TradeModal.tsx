//! 股票买卖弹窗。
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import { localDateTimeValue } from "../../lib";
import type { Account } from "../../types";

export function TradeModal({
  accounts,
  initialSide,
  initialSymbol,
  onClose,
  onSubmit
}: {
  accounts: Account[];
  initialSide: "buy" | "sell";
  initialSymbol: string;
  onClose: () => void;
  onSubmit: (input: {
    side: "buy" | "sell";
    payload: {
      account_id: number;
      symbol: string;
      shares: string;
      price: string;
      occurred_at?: string;
      note?: string;
    };
  }) => Promise<void>;
}) {
  // 持仓由买入交易创建；无需先建股票账户。保留股票账户兼容独立管理券商余额的用户。
  const fundingAccounts = accounts.filter((account) => account.account_type !== "credit");
  const [side, setSide] = useState<"buy" | "sell">(initialSide);
  const [accountId, setAccountId] = useState(fundingAccounts[0]?.id ?? 0);
  const [symbol, setSymbol] = useState(initialSymbol);
  const [shares, setShares] = useState("");
  const [price, setPrice] = useState("");
  const [note, setNote] = useState("");
  const [occurredAt, setOccurredAt] = useState(localDateTimeValue);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      await onSubmit({
        side,
        payload: {
          account_id: Number(accountId),
          symbol,
          shares,
          price,
          occurred_at: new Date(occurredAt).toISOString(),
          note: note || undefined
        }
      });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("common.opFailed"));
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="TRADE" title={t(side === "buy" ? "modals.trade.titleBuy" : "modals.trade.titleSell")} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="kind-tabs">
          {(["buy", "sell"] as const).map((item) => (
            <button type="button" key={item} className={side === item ? "active" : ""} onClick={() => setSide(item)}>
              {t(item === "buy" ? "modals.trade.buy" : "modals.trade.sell")}
            </button>
          ))}
        </div>
        <div className="form-grid">
          <label><span>{t("modals.trade.fundingAccount")}</span>
            <select required value={accountId} onChange={(event) => setAccountId(Number(event.target.value))}>
              {fundingAccounts.length === 0 && <option value={0} disabled>{t("modals.trade.noFundingAccount")}</option>}
              {fundingAccounts.map((account) => (
                <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>
              ))}
            </select>
          </label>
          <p className="fx-hint span-two">{t("modals.trade.fundingHint")}</p>
          <label><span>{t("modals.trade.symbol")}</span><input required autoFocus value={symbol} onChange={(event) => setSymbol(event.target.value)} placeholder={t("modals.trade.symbolPlaceholder")} /></label>
          <label><span>{t("modals.trade.shares")}</span><input required min="0.0001" step="0.0001" inputMode="decimal" value={shares} onChange={(event) => setShares(event.target.value)} placeholder="0" /></label>
          <label><span>{t("modals.trade.price")}</span><input required min="0.01" step="0.01" inputMode="decimal" value={price} onChange={(event) => setPrice(event.target.value)} placeholder="0.00" /></label>
          <label><span>{t("common.time")}</span><input type="datetime-local" value={occurredAt} onChange={(event) => setOccurredAt(event.target.value)} /></label>
          <label className="span-two"><span>{t("common.note")}</span><input value={note} onChange={(event) => setNote(event.target.value)} placeholder={t("common.optional")} /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
          <button className="primary-button" disabled={submitting || !symbol || !shares || !price || !accountId}>{submitting && <LoaderCircle className="spin" size={17} />}{t(side === "buy" ? "modals.trade.buy" : "modals.trade.sell")}</button>
        </div>
      </form>
    </ModalShell>
  );
}
