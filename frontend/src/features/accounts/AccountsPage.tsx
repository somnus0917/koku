//! 账户页：资产/负债汇总、账户分组与定存/借款/周期/持仓区块。
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { MoreHorizontal, Plus } from "lucide-react";
import { PageTitle } from "../../components/PageTitle";
import { EmptyState } from "../../components/EmptyState";
import { SummaryCard } from "../../components/SummaryCard";
import { accountIcon, convertedMoney, useConversionRates } from "../../components/accountDisplay";
import { DepositSection } from "../deposits/DepositSection";
import { LoansSection } from "../loans/LoansSection";
import { RecurringSection } from "../recurring/RecurringSection";
import { HoldingSection } from "../holdings/HoldingSection";
import { CreditCardSection } from "./CreditCardSection";
import { formatMoney } from "../../lib";
import type { Account, AccountType, AppData, Deposit, Loan } from "../../types";

export function AccountsPage({
  data,
  onAddAccount,
  onEdit,
  onDeposit,
  onSettle,
  onCreateLoan,
  onRepay,
  onCreateRecurring,
  onDeleteRecurring,
  onBuyStock,
  onSellStock,
  onSetHoldingPrice,
  onReconcile,
  onRefreshHoldings,
  onRefreshHolding
}: {
  data: AppData;
  onAddAccount: () => void;
  onEdit: (account: Account) => void;
  onDeposit: (account: Account) => void;
  onSettle: (deposit: Deposit) => void;
  onCreateLoan: () => void;
  onRepay: (loan: Loan) => void;
  onCreateRecurring: () => void;
  onDeleteRecurring: (id: number) => void;
  onBuyStock: (symbol?: string) => void;
  onSellStock: (symbol: string) => void;
  onSetHoldingPrice: (holdingId: number, price: string) => void;
  /** 打开某账户的对账弹窗。 */
  onReconcile: (account: Account) => void;
  /** 刷新过期/缺失市价（懒拉取），透传给持仓区块。 */
  onRefreshHoldings: () => void;
  /** 强制刷新单只持仓市价（可选）。 */
  onRefreshHolding?: (holdingId: number) => void;
}) {
  const group = (type: AccountType) => data.accounts.filter((account) => account.account_type === type);
  const cash = group("cash");
  const savings = group("savings");
  const stock = group("stock");
  const credit = group("credit");
  const display = data.monthly.currency;
  const rateCurrencies = useMemo(
    () => [
      ...new Set([
        ...data.accounts.map((account) => account.currency),
        ...data.loans.map((loan) => loan.currency)
      ])
    ],
    [data.accounts, data.loans]
  );
  const rates = useConversionRates(rateCurrencies, display);
  const { t } = useTranslation();
  return (
    <div className="page page-enter">
      <PageTitle
        eyebrow="ACCOUNTS"
        title={t("nav.accounts")}
        actions={<button className="primary-button" onClick={onAddAccount}><Plus size={18} /> {t("accounts.newAccount")}</button>}
      />
      <section className="balance-summary-row">
        <SummaryCard label={t("accounts.totalAssets")} value={data.balance.total_assets} currency={data.balance.currency} tone="green" />
        <SummaryCard label={t("accounts.totalLiabilities")} value={data.balance.total_liabilities} currency={data.balance.currency} tone="orange" />
        <SummaryCard label={t("accounts.netWorth")} value={data.balance.net_worth} currency={data.balance.currency} tone="blue" />
      </section>
      <AccountGroup title={t("accounts.type.cash")} subtitle={t("accounts.accountCount", { count: cash.length })} accounts={cash} onEdit={onEdit} onReconcile={onReconcile} display={display} rates={rates} />
      <AccountGroup title={t("accounts.type.savings")} subtitle={t("accounts.accountCount", { count: savings.length })} accounts={savings} onEdit={onEdit} onReconcile={onReconcile} display={display} rates={rates} />
      <DepositSection
        deposits={data.deposits}
        accounts={data.accounts}
        display={display}
        rates={rates}
        onDeposit={onDeposit}
        onSettle={onSettle}
      />
      <AccountGroup title={t("accounts.type.stock")} subtitle={t("accounts.accountCount", { count: stock.length })} accounts={stock} onEdit={onEdit} onReconcile={onReconcile} display={display} rates={rates} />
      <AccountGroup title={t("accounts.type.credit")} subtitle={t("accounts.accountCount", { count: credit.length })} accounts={credit} onEdit={onEdit} onReconcile={onReconcile} display={display} rates={rates} />
      <CreditCardSection accounts={credit} display={display} rates={rates} data={data} />
      <LoansSection
        loans={data.loans}
        accounts={data.accounts}
        display={display}
        rates={rates}
        onCreateLoan={onCreateLoan}
        onRepay={onRepay}
      />
      <RecurringSection
        rules={data.recurring}
        accounts={data.accounts}
        categories={data.categories}
        onCreate={onCreateRecurring}
        onDelete={onDeleteRecurring}
      />
      <HoldingSection
        holdings={data.holdings}
        accounts={data.accounts}
        display={display}
        rates={rates}
        onBuy={onBuyStock}
        onSell={onSellStock}
        onSetPrice={onSetHoldingPrice}
        onRefreshHoldings={onRefreshHoldings}
        onRefreshHolding={onRefreshHolding}
      />
    </div>
  );
}
export function AccountGroup({
  title,
  subtitle,
  accounts,
  onEdit,
  onReconcile,
  display,
  rates
}: {
  title: string;
  subtitle: string;
  accounts: Account[];
  onEdit?: (account: Account) => void;
  /** 传入后账户卡片出现「对账」入口。 */
  onReconcile?: (account: Account) => void;
  /** 显示币种（右上角切换）；传入后余额/额度按汇率折算显示，并标注原币 */
  display: string;
  /** 折算汇率表：账户币种 → 1 unit = factor display */
  rates: Record<string, number>;
}) {
  const { t } = useTranslation();
  return (
    <section className="section-block account-group">
      <div className="section-heading compact-heading">
        <div><span>{subtitle}</span><h2>{title}</h2></div>
      </div>
      <div className="account-grid">
        {accounts.map((account, index) => {
          const Icon = accountIcon(account);
          const shown = convertedMoney(account.balance, account.currency, display, rates)
            ?? { amount: account.balance, currency: account.currency };
          const isConverted = shown.currency !== account.currency;
          const limitShown = account.credit_limit
            ? convertedMoney(account.credit_limit, account.currency, display, rates)
            : null;
          return (
            <article className="account-detail-card" key={account.id}>
              <span className={`large-account-icon tone-${index % 4}`}><Icon size={23} /></span>
              <div className="account-detail-copy">
                <h3>{account.name}</h3>
                <span>
                  {isConverted ? `${t("accounts.originalCurrency", { amount: formatMoney(account.balance, account.currency) })} · ` : ""}
                  {account.credit_limit
                    ? `${t("accounts.limitLabel")} ${formatMoney(limitShown?.amount ?? account.credit_limit, limitShown?.currency ?? account.currency)}`
                    : ""}
                </span>
              </div>
              <strong>{formatMoney(shown.amount, shown.currency)}</strong>
              <div className="account-card-actions">
                {onReconcile && <button className="text-button" onClick={() => onReconcile(account)}>{t("reconcile.start")}</button>}
                <button className="bare-button" aria-label={t("accounts.editAria", { name: account.name })} title={t("accounts.editTitle")} onClick={() => onEdit?.(account)}><MoreHorizontal size={19} /></button>              </div>
            </article>
          );
        })}
        {accounts.length === 0 && <EmptyState title={t("accounts.emptyTitle")} detail={t("accounts.emptyDetail")} />}
      </div>
    </section>
  );
}
