//! 简洁流水列表（总览页近期流水）。
import { useTranslation } from "react-i18next";
import { EmptyState } from "../../components/EmptyState";
import { TransactionRow } from "./TransactionRow";
import type { Account, Category, Transaction } from "../../types";

export function TransactionList({
  transactions,
  accounts,
  categories,
  display,
  rates
}: {
  transactions: Transaction[];
  accounts: Account[];
  categories: Category[];
  /** 显示币种（右上角切换）；传入后金额按汇率折算显示 */
  display?: string;
  /** 折算汇率表：交易币种 → 1 unit = factor display */
  rates?: Record<string, number>;
}) {
  const accountMap = new Map(accounts.map((item) => [item.id, item]));
  const categoryMap = new Map(categories.map((item) => [item.id, item]));
  const { t } = useTranslation();
  return (
    <div className="simple-list">
      {transactions.map((transaction) => (
        <TransactionRow
          compact
          key={transaction.id}
          transaction={transaction}
          account={accountMap.get(transaction.account_id)}
          target={transaction.to_account_id ? accountMap.get(transaction.to_account_id) : undefined}
          category={transaction.category_id ? categoryMap.get(transaction.category_id) : undefined}
          display={display}
          rates={rates}
        />
      ))}
      {transactions.length === 0 && <EmptyState title={t("transactions.emptyTitle")} detail={t("transactions.emptyDetail")} />}
    </div>
  );
}
