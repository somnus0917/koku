//! 借款/出借区块（账户页）：未结与已结借款列表。
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Handshake, Plus, RefreshCcw } from "lucide-react";
import { SummaryCard } from "../../components/SummaryCard";
import { EmptyState } from "../../components/EmptyState";
import { convertedMoney } from "../../components/accountDisplay";
import { formatDate, formatMoney } from "../../lib";
import type { Account, Loan } from "../../types";

export function LoansSection({
  loans,
  accounts,
  display,
  rates,
  onCreateLoan,
  onRepay
}: {
  loans: Loan[];
  accounts: Account[];
  /** 显示币种（右上角切换）；传入后本金/未结按汇率折算显示 */
  display: string;
  /** 折算汇率表：借款币种 → 1 unit = factor display */
  rates: Record<string, number>;
  onCreateLoan: () => void;
  onRepay: (loan: Loan) => void;
}) {
  const accountMap = useMemo(() => new Map(accounts.map((account) => [account.id, account])), [accounts]);
  const open = loans.filter((loan) => !loan.closed_at);
  const closed = loans.filter((loan) => loan.closed_at);
  const shown = (value: string, from: string) =>
    convertedMoney(value, from, display, rates) ?? { amount: value, currency: from };
  const totalDue = (loan: Loan) => String(Number(loan.outstanding) + Number(loan.accrued_interest));
  const lendOutstanding = open
    .filter((loan) => loan.loan_type === "lend")
    .reduce((sum, loan) => sum + Number(shown(totalDue(loan), loan.currency).amount), 0);
  const borrowOutstanding = open
    .filter((loan) => loan.loan_type === "borrow")
    .reduce((sum, loan) => sum + Number(shown(totalDue(loan), loan.currency).amount), 0);
  const { t } = useTranslation();
  return (
    <section className="section-block account-group">
      <div className="section-heading compact-heading">
        <div><span>LOANS</span><h2>{t("accounts.loans.title")}</h2></div>
        <button className="text-button" onClick={onCreateLoan}><Plus size={16} /> {t("accounts.loans.add")}</button>
      </div>
      <div className="balance-summary-row">
        <SummaryCard label={t("accounts.loans.receivable")} value={lendOutstanding.toFixed(2)} currency={display} tone="green" />
        <SummaryCard label={t("accounts.loans.payable")} value={borrowOutstanding.toFixed(2)} currency={display} tone="orange" />
      </div>
      <div className="account-grid">
        {open.map((loan) => {
          const outstandingShown = shown(totalDue(loan), loan.currency);
          const principalShown = shown(loan.principal, loan.currency);
          const isConverted = outstandingShown.currency !== loan.currency;
          return (
            <article className="account-detail-card" key={loan.id}>
              <span className={`large-account-icon tone-${loan.id % 4}`}><Handshake size={23} /></span>
              <div className="account-detail-copy">
                <h3>
                  {loan.counterparty}
                  <small className={loan.loan_type === "lend" ? "income-text" : "expense-text"}>
                    {t(loan.loan_type === "lend" ? "common.lend" : "common.borrow")}
                  </small>
                </h3>
                <span>
                  {loan.currency}
                  {isConverted ? `（${t("accounts.originalCurrency", { amount: formatMoney(totalDue(loan), loan.currency) })}）` : ""} · {t("accounts.loans.principal")}{" "}
                  {formatMoney(principalShown.amount, principalShown.currency)}
                  {isConverted ? `（${t("accounts.originalCurrency", { amount: formatMoney(loan.principal, loan.currency) })}）` : ""} ·{" "}
                  {accountMap.get(loan.account_id)?.name ?? t("common.unknownAccount")}
                  {loan.interest_rate ? ` · ${t("accounts.loans.interest", { rate: loan.interest_rate, amount: formatMoney(loan.accrued_interest, loan.currency) })}` : ""}
                  {loan.note ? ` · ${loan.note}` : ""}
                </span>
              </div>
              <strong>{formatMoney(outstandingShown.amount, outstandingShown.currency)}</strong>
              <button className="row-action" onClick={() => onRepay(loan)} title={t("accounts.loans.repay")} aria-label={t("accounts.loans.repay")}><RefreshCcw size={16} /></button>
            </article>
          );
        })}
        {open.length === 0 && <EmptyState title={t("accounts.loans.emptyTitle")} detail={t("accounts.loans.emptyDetail")} />}
      </div>
      {closed.length > 0 && (
        <div className="account-grid closed-loans">
          {closed.map((loan) => (
            <article className="account-detail-card muted" key={loan.id}>
              <span className="large-account-icon"><Handshake size={23} /></span>
              <div className="account-detail-copy">
                <h3>{loan.counterparty}<small>{t(loan.loan_type === "lend" ? "common.lend" : "common.borrow")}</small></h3>
                <span>{formatDate(loan.opened_at)} {t("common.opened")}{loan.closed_at ? ` · ${formatDate(loan.closed_at)} ${t("common.settled")}` : ""}{loan.interest_rate ? ` · ${t("accounts.loans.settledInterest", { amount: formatMoney(loan.accrued_interest, loan.currency) })}` : ""}</span>
              </div>
              <strong>{t("common.settledDone")}</strong>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}
