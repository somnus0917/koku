//! 定期存款区块（账户页）：未结与已结定存列表。
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { PiggyBank, RotateCcw } from "lucide-react";
import { EmptyState } from "../../components/EmptyState";
import { convertedMoney } from "../../components/accountDisplay";
import { formatDate, formatMoney } from "../../lib";
import type { Account, Deposit } from "../../types";

export function DepositSection({
  deposits,
  accounts,
  display,
  rates,
  onDeposit,
  onSettle
}: {
  deposits: Deposit[];
  accounts: Account[];
  display: string;
  rates: Record<string, number>;
  onDeposit: (account: Account) => void;
  onSettle: (deposit: Deposit) => void;
}) {
  const savings = accounts.filter((account) => account.account_type === "savings");
  const open = deposits.filter((deposit) => !deposit.settled_at);
  const closed = deposits.filter((deposit) => deposit.settled_at);
  const accountMap = useMemo(() => new Map(accounts.map((account) => [account.id, account])), [accounts]);
  const shown = (value: string, from: string) =>
    convertedMoney(value, from, display, rates) ?? { amount: value, currency: from };
  const { t } = useTranslation();
  return (
    <section className="section-block account-group">
      <div className="section-heading compact-heading">
        <div><span>DEPOSITS</span><h2>{t("deposit.title")}</h2></div>
        {savings.length > 0 && (
          <button className="text-button" onClick={() => onDeposit(savings[0])}><PiggyBank size={16} /> {t("deposit.convert")}</button>
        )}
      </div>
      <div className="account-grid">
        {open.map((deposit) => {
          const principal = shown(deposit.amount, deposit.currency);
          return (
            <article className="account-detail-card" key={deposit.id}>
              <span className="large-account-icon tone-1"><PiggyBank size={23} /></span>
              <div className="account-detail-copy">
                <h3>{t("deposit.term", { days: deposit.term_days })}</h3>
                <span>
                  {t("deposit.meta", { rate: deposit.rate, date: formatDate(deposit.maturity_at), account: accountMap.get(deposit.source_account_id)?.name ?? t("common.unknownAccount") })}
                </span>
              </div>
              <strong>{formatMoney(principal.amount, principal.currency)}</strong>
              <button className="row-action" onClick={() => onSettle(deposit)} title={t("deposit.settleTitle")} aria-label={t("deposit.settleAria")}><RotateCcw size={16} /></button>
            </article>
          );
        })}
        {open.length === 0 && <EmptyState title={t("deposit.emptyTitle")} detail={t("deposit.emptyDetail")} />}
      </div>
      {closed.length > 0 && (
        <div className="account-grid closed-loans">
          {closed.map((deposit) => (
            <article className="account-detail-card muted" key={deposit.id}>
              <span className="large-account-icon"><PiggyBank size={23} /></span>
              <div className="account-detail-copy">
                <h3>{t("deposit.term", { days: deposit.term_days })}</h3>
                <span>{formatDate(deposit.opened_at)} {t("common.opened")}{deposit.settled_at ? ` · ${formatDate(deposit.settled_at)} ${t("common.settled")}` : ""}</span>
              </div>
              <strong>{t("common.settledDone")}</strong>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}
