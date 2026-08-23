//! 票根墙：把流水以固定倾角的票据呈现，突出金额大小与每笔交易的独立感。
import type { CSSProperties } from "react";
import { Paperclip, Pencil } from "lucide-react";
import { useTranslation } from "react-i18next";
import { CategoryAvatar } from "../../components/avatar";
import { formatDate, formatMoney } from "../../lib";
import type { Account, Category, Transaction } from "../../types";

type ReceiptSize = "standard" | "medium" | "large";

/** 同一交易始终得到同一角度，避免刷新时票据“跳动”。 */
function ticketTilt(id: number): number {
  return (((id * 17) % 9) - 4) * 0.38;
}

function ticketSize(transaction: Transaction, largestAmount: number, count: number): ReceiptSize {
  if (count < 3 || largestAmount <= 0) return "standard";
  const relative = Math.abs(Number(transaction.amount)) / largestAmount;
  if (relative >= 0.70) return "large";
  if (relative >= 0.28) return "medium";
  return "standard";
}

function receiptDay(value: string): string {
  return value.slice(0, 10);
}

function receiptTime(value: string): string {
  return new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit", hour12: false }).format(new Date(value));
}

export function ReceiptWall({
  transactions,
  accountsById,
  categoriesById,
  onEdit
}: {
  transactions: Transaction[];
  accountsById: Map<number, Account>;
  categoriesById: Map<number, Category>;
  onEdit: (transaction: Transaction) => void;
}) {
  const { t } = useTranslation();
  const largestAmount = Math.max(0, ...transactions.map((item) => Math.abs(Number(item.amount)) || 0));
  const days = transactions.reduce<Array<{ day: string; transactions: Transaction[] }>>((groups, transaction) => {
    const day = receiptDay(transaction.occurred_at);
    const current = groups.at(-1);
    if (current?.day === day) current.transactions.push(transaction);
    else groups.push({ day, transactions: [transaction] });
    return groups;
  }, []);
  return (
    <section className="receipt-wall" aria-label={t("transactions.viewMode.receipts")}>
      {days.map((group) => <section className="receipt-day" key={group.day}>
        <h2>{formatDate(`${group.day}T12:00:00`)}</h2>
        <div className="receipt-day-grid">
          {group.transactions.map((transaction) => {
            const category = transaction.category_id ? categoriesById.get(transaction.category_id) : undefined;
            const account = accountsById.get(transaction.account_id);
            const income = transaction.kind === "income";
            const expense = transaction.kind === "expense";
            const typeLabel = t(`transactions.meta.${transaction.kind}`);
            const title = transaction.payee_name || transaction.note || category?.name || typeLabel;
            const subtitle = category?.name || typeLabel;
            const size = ticketSize(transaction, largestAmount, transactions.length);
            const prefix = expense ? "−" : income ? "+" : "";
            const proportion = largestAmount > 0 ? Math.max(4, Math.round((Math.abs(Number(transaction.amount)) / largestAmount) * 100)) : 4;
            return (
              <article
                className={`receipt-ticket ${size} ${expense ? "expense" : income ? "income" : "transfer"} ${transaction.voided_at ? "voided" : ""}`}
                key={transaction.id}
                style={{ "--ticket-rotation": `${ticketTilt(transaction.id)}deg`, "--ticket-proportion": `${proportion}%` } as CSSProperties}
              >
                <span className="receipt-pin" aria-hidden="true" />
                <header>
                  <CategoryAvatar name={title} icon={category?.icon} className="receipt-category-avatar" />
                  <time dateTime={transaction.occurred_at}>{receiptTime(transaction.occurred_at)}</time>
                </header>
                <div className="receipt-ticket-copy"><strong>{title}</strong><small>{subtitle}</small></div>
                <div className="receipt-ticket-total"><span>{t("transactions.receiptWall.total")}</span><strong>{prefix}{formatMoney(transaction.amount, transaction.currency)}</strong><i aria-hidden="true" /></div>
                <footer>
                  <span>{account?.name ?? t("common.unknownAccount")}</span>
                  <div>
                    {transaction.has_receipt && <span className="receipt-paperclip"><Paperclip size={13} /> {t("transactions.receipt")}</span>}
                    <button type="button" onClick={() => onEdit(transaction)} aria-label={t("transactions.edit")} title={t("transactions.edit")}><Pencil size={14} /></button>
                  </div>
                </footer>
              </article>
            );
          })}
        </div>
      </section>)}
    </section>
  );
}
