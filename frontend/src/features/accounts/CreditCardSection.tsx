//! 信用卡区域：每张信用卡的额度占用、出账/未出账与账单/还款日（v1 简单统计块）。
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { CreditCard } from "lucide-react";
import { convertedMoney } from "../../components/accountDisplay";
import { formatDay, formatMoney } from "../../lib";
import { getCreditCardStatements, getCreditCardSummary } from "../../api";
import type { Account, AppData, CreditCardStatement, CreditCardSummary } from "../../types";

export function CreditCardSection({
  accounts,
  display,
  rates,
  data
}: {
  accounts: Account[];
  display: string;
  rates: Record<string, number>;
  /** 账本数据快照：每次 ledger refresh 都会生成新对象，作为摘要刷新信号。 */
  data: AppData;
}) {
  const { t } = useTranslation();
  if (accounts.length === 0) return null;
  return (
    <section className="section-block credit-card-section">
      <div className="section-heading compact-heading">
        <div><span>{t("accounts.accountCount", { count: accounts.length })}</span><h2>{t("accounts.creditCards")}</h2></div>
      </div>
      <div className="credit-card-grid">
        {accounts.map((account) => (
          <CreditCardCard key={account.id} account={account} display={display} rates={rates} data={data} />
        ))}
      </div>
    </section>
  );
}

function CreditCardCard({
  account,
  display,
  rates,
  data
}: {
  account: Account;
  display: string;
  rates: Record<string, number>;
  data: AppData;
}) {
  const { t } = useTranslation();
  const [summary, setSummary] = useState<CreditCardSummary | null>(null);
  const [statements, setStatements] = useState<CreditCardStatement[]>([]);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let cancelled = false;
    setFailed(false);
    Promise.all([getCreditCardSummary(account.id), getCreditCardStatements(account.id)])
      .then(([item, history]) => {
        if (!cancelled) { setSummary(item); setStatements(history); }
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
    };
    // 消费/还款/void/修改 occurred_at/修改账单日等都会触发 ledger refresh，
    // data 引用随之变化 → 重新请求摘要（不轮询、不引入全局状态）。
  }, [account.id, data]);

  const money = (value: string | null) => {
    if (value == null) return null;
    const converted = convertedMoney(value, account.currency, display, rates);
    return converted ?? { amount: value, currency: account.currency };
  };
  const row = (label: string, value: string | null) =>
    value == null ? null : (
      <div className="credit-card-stat">
        <span>{label}</span>
        <strong>{value}</strong>
      </div>
    );

  const limit = money(summary?.credit_limit ?? null);
  const used = money(summary?.used_credit ?? null);
  const available = money(summary?.available_credit ?? null);
  const current = money(summary?.current_statement_amount ?? null);
  const unbilled = money(summary?.unbilled_amount ?? null);

  return (
    <article className="credit-card-card">
      <div className="credit-card-head">
        <span className="credit-card-icon"><CreditCard size={17} /></span>
        <strong>{account.name}</strong>
        {summary && (
          <span className="credit-card-balance">{formatMoney(account.balance, account.currency)}</span>
        )}
      </div>
      {failed ? (
        <p className="fx-hint">{t("accounts.creditSummaryFailed")}</p>
      ) : !summary ? (
        <p className="fx-hint">{t("accounts.creditSummaryLoading")}</p>
      ) : (
        <>
          <div className="credit-card-stats">
            {row(t("accounts.creditLimit"), limit ? formatMoney(limit.amount, limit.currency) : null)}
            {row(t("accounts.usedCredit"), used ? formatMoney(used.amount, used.currency) : null)}
            {row(t("accounts.availableCredit"), available ? formatMoney(available.amount, available.currency) : null)}
            {row(t("accounts.currentStatement"), current ? formatMoney(current.amount, current.currency) : null)}
            {row(t("accounts.unbilled"), unbilled ? formatMoney(unbilled.amount, unbilled.currency) : null)}
          </div>
          <div className="credit-card-dates">
            {summary.statement_day != null && row(
              t("accounts.statementDay"),
              t("accounts.statementDayMonthly", { day: summary.statement_day })
            )}
            {summary.due_day != null && row(t("accounts.dueDay"), t("accounts.statementDayMonthly", { day: summary.due_day }))}
            {summary.next_statement_date != null && row(t("accounts.nextStatement"), formatDay(summary.next_statement_date))}
            {summary.next_due_date != null && row(t("accounts.nextDue"), formatDay(summary.next_due_date))}
          </div>
          {statements.length > 0 && (
            <div className="credit-card-dates" aria-label="Credit card statement history">
              {statements.slice(0, 3).map((item) => (
                <div className="credit-card-stat" key={item.statement_date}>
                  <span>{formatDay(item.statement_date)} · {item.due_at ? formatDay(item.due_at) : "—"}</span>
                  <strong>{formatMoney(item.outstanding, account.currency)} / {formatMoney(item.amount, account.currency)}</strong>
                </div>
              ))}
            </div>
          )}
        </>
      )}
    </article>
  );
}
