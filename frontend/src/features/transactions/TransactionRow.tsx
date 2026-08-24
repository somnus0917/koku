//! 流水行：金额/币种折算展示、作废/恢复/永久删除、报销与小票操作。
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDownLeft, ArrowLeftRight, ArrowUpRight, BadgeDollarSign, CircleCheck, Handshake, MoreHorizontal, Paperclip, PiggyBank, RotateCcw, Tags, Trash2, TrendingUp, X } from "lucide-react";
import { CategoryAvatar } from "../../components/avatar";
import { receiptUrl } from "../../api";
import { formatDate, formatMoney } from "../../lib";
import type { Account, Category, Transaction } from "../../types";

export function TransactionRow({
  transaction,
  account,
  target,
  category,
  compact = false,
  display,
  rates,
  onVoid,
  onRestore,
  onDeletePermanently,
  onMarkReimbursable,
  onUnmarkReimbursable,
  onReimburse,
  onRefund,
  onEdit,
  onUploadReceipt
}: {
  transaction: Transaction;
  account?: Account;
  target?: Account;
  category?: Category;
  compact?: boolean;
  /** 显示币种（右上角切换）；传入后金额按汇率折算显示，原币金额保留为辅助行 */
  display?: string;
  /** 折算汇率表：交易币种 → 1 unit = factor display；缺汇率的币种回退原币显示 */
  rates?: Record<string, number>;
  onVoid?: () => void;
  /** 已撤销的流水：恢复（撤销删除） */
  onRestore?: () => void;
  /** 已撤销的流水：永久删除（不可恢复） */
  onDeletePermanently?: () => void;
  onMarkReimbursable?: () => void;
  onUnmarkReimbursable?: () => void;
  onReimburse?: () => void;
  onRefund?: () => void;
  /** 传入后在行最右侧显示 ⋯ 菜单（编辑交易） */
  onEdit?: () => void;
  /** 传入后菜单里出现「上传小票」 */
  onUploadReceipt?: (file: File) => void;
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const fileRef = useRef<HTMLInputElement | null>(null);
  // 点击菜单外部时关闭。
  useEffect(() => {
    if (!menuOpen) return;
    const close = (event: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        setMenuOpen(false);
      }
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [menuOpen]);
  const { t } = useTranslation();
  const meta = {
    expense: { icon: ArrowUpRight, label: category?.name ?? t("transactions.kind.expense"), className: "expense" },
    income: { icon: ArrowDownLeft, label: category?.name ?? t("transactions.kind.income"), className: "income" },
    transfer: { icon: ArrowLeftRight, label: t("transactions.meta.transfer"), className: "transfer" },
    loan: { icon: Handshake, label: transaction.note || t("transactions.meta.loan"), className: "transfer" },
    adjustment: { icon: RotateCcw, label: t("transactions.meta.adjustment"), className: "transfer" },
    trade: { icon: TrendingUp, label: t("transactions.meta.trade"), className: "transfer" },
    deposit: { icon: PiggyBank, label: t("transactions.meta.deposit"), className: "transfer" }
  }[transaction.kind];
  const Icon = meta.icon;
  const prefix =
    transaction.kind === "expense" ? "−"
    : transaction.kind === "income" ? "+"
    : transaction.kind === "adjustment" || transaction.kind === "trade" || transaction.kind === "deposit"
      ? (Number(transaction.amount) > 0 ? "+" : "")
    : "";
  const reimbursable = transaction.reimbursable_at && !transaction.reimbursed_at;
  const hasReimburseActions = transaction.kind === "expense" && !transaction.voided_at && !transaction.reimbursed_at;
  const canRefund = transaction.kind === "expense" && !transaction.voided_at && Number(transaction.amount) > Number(transaction.reimbursed_amount) + Number(transaction.refunded_amount);

  // 折算显示：display 币种与交易币种不同且有汇率时，主金额换算为显示币种，
  // 并用一行「原币」保留真实入账金额；无汇率时回退原币显示。
  const factor = display && transaction.currency !== display ? rates?.[transaction.currency] : undefined;
  const converted = factor != null;
  const mainAmount = converted ? (Number(transaction.amount) * factor!).toFixed(2) : transaction.amount;
  const mainCurrency = converted ? display! : transaction.currency;
  const targetFactor =
    display && transaction.target_currency && transaction.target_currency !== display
      ? rates?.[transaction.target_currency]
      : undefined;
  const targetConverted = targetFactor != null && transaction.target_amount != null;
  const targetAmount = targetConverted
    ? (Number(transaction.target_amount) * targetFactor!).toFixed(2)
    : transaction.target_amount;
  const targetCurrency = targetConverted ? display! : transaction.target_currency;
  const reimbursedShown = converted
    ? formatMoney((Number(transaction.reimbursed_amount) * factor!).toFixed(2), display!)
    : formatMoney(transaction.reimbursed_amount, transaction.currency);
  const refundedShown = converted
    ? formatMoney((Number(transaction.refunded_amount) * factor!).toFixed(2), display!)
    : formatMoney(transaction.refunded_amount, transaction.currency);
  // 普通收支且设置了商户：主标题展示商户，meta 展示「分类 · 备注」。
  const showPayee =
    (transaction.kind === "expense" || transaction.kind === "income") &&
    Boolean(transaction.payee_name);
  const payeeMeta = showPayee
    ? [meta.label, transaction.note].filter(Boolean).join(" · ")
    : null;
  return (
    <div className={`transaction-row ${compact ? "compact-row" : ""} ${transaction.voided_at ? "voided" : ""}`}>
      <div className="transaction-main">
        {transaction.kind === "transfer" || transaction.kind === "loan" || transaction.kind === "adjustment" || transaction.kind === "trade" || transaction.kind === "deposit" ? (
          <span className={`transaction-icon ${meta.className}`}><Icon size={18} /></span>
        ) : (
          <CategoryAvatar name={showPayee ? transaction.payee_name! : meta.label} icon={showPayee ? null : category?.icon} className={`transaction-icon ${meta.className}`} />
        )}
        <div>
          <strong>
            {showPayee ? transaction.payee_name : transaction.note || meta.label}
            {transaction.voided_at ? t("transactions.voidedSuffix") : ""}
          </strong>
          <span className="transaction-meta">
            <span>{payeeMeta ?? meta.label}</span>
            {reimbursable ? <span className="reimburse-status">{t("transactions.pendingReimburse")}</span> : ""}
            {transaction.has_splits ? <span className="split-status">{t("transactions.split")}</span> : ""}
            {transaction.has_receipt ? <span className="receipt-status"><Paperclip size={11} /> {t("transactions.receipt")}</span> : ""}
            {transaction.tags.map((tag) => (
              <span className="transaction-tag" key={tag}>#{tag}</span>
            ))}
          </span>
        </div>
      </div>
      {!compact && <span className="table-account">{account?.name ?? t("common.unknownAccount")}{target ? ` → ${target.name}` : ""}</span>}
      {!compact && <span className="table-date">{formatDate(transaction.occurred_at)}</span>}
      <div className={`transaction-amount ${meta.className}`}>
        <strong>{prefix}{formatMoney(mainAmount, mainCurrency)}</strong>
        {converted && <span>{t("transactions.original")} {formatMoney(transaction.amount, transaction.currency)}</span>}
        {transaction.kind === "transfer" && targetAmount && targetCurrency && (
          <span>{t("transactions.arrived")} {formatMoney(targetAmount, targetCurrency)}</span>
        )}
        {!converted && transaction.kind !== "transfer" && transaction.kind !== "loan" && transaction.kind !== "adjustment" && account && transaction.currency !== account.currency && (
          <span>{t("transactions.settledLabel")} {formatMoney(transaction.settled_amount, account.currency)}</span>
        )}
        {transaction.kind === "expense" && transaction.reimbursed_amount !== "0" && !transaction.reimbursed_at && (
          <span>{t("transactions.reimbursedLabel")} {reimbursedShown}</span>
        )}
        {transaction.kind === "expense" && transaction.refunded_amount !== "0" && (
          <span>{t("transactions.refundedLabel")} {refundedShown}</span>
        )}
        {compact && <span>{formatDate(transaction.occurred_at)}</span>}
      </div>
      {!compact && (
        <div className="transaction-actions">
          {hasReimburseActions && (
            reimbursable
              ? <>
                  <button className="row-action reimburse" onClick={onReimburse} title={t("transactions.reimburse")} aria-label={t("transactions.reimburse")}><BadgeDollarSign size={16} /></button>
                  <button className="row-action reimburse" onClick={onUnmarkReimbursable} title={t("transactions.unmarkReimburse")} aria-label={t("transactions.unmarkReimburse")}><X size={16} /></button>
                </>
              : <button className="row-action reimburse" onClick={onMarkReimbursable} title={t("transactions.markReimburse")} aria-label={t("transactions.markReimburse")}><Tags size={16} /></button>
          )}
          {canRefund && <button className="row-action reimburse" onClick={onRefund} title={t("transactions.refund")} aria-label={t("transactions.refund")}><RotateCcw size={16} /></button>}
          {transaction.reimbursed_at && (
            <span
              className="reimbursed-indicator"
              title={t("transactions.reimbursedTitle", { amount: reimbursedShown })}
              aria-label={t("transactions.reimbursedTitle", { amount: reimbursedShown })}
            ><CircleCheck size={16} /></span>
          )}
          {transaction.voided_at
            ? onRestore && (
                <button
                  className="row-action"
                  onClick={onRestore}
                  title={t("transactions.restoreTitle")}
                  aria-label={t("transactions.restore")}
                ><RotateCcw size={16} /></button>
              )
            : (
                <button
                  className="row-action"
                  disabled={transaction.kind === "loan" || transaction.kind === "trade" || transaction.kind === "deposit"}
                  onClick={onVoid}
                  title={t("transactions.voidTitle")}
                  aria-label={t("transactions.voidAria")}
                ><Trash2 size={16} /></button>
              )}
        </div>
      )}
      {(onEdit || onRestore || onDeletePermanently) && (
        <div className="row-menu-wrap" ref={menuRef}>
          <button
            type="button"
            className={`row-action ${menuOpen ? "active" : ""}`}
            onClick={() => setMenuOpen((open) => !open)}
            title={t("transactions.moreActions")}
            aria-label={t("transactions.moreActions")}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
          ><MoreHorizontal size={16} /></button>
          {menuOpen && (
            <div className="row-menu" role="menu">
              {transaction.voided_at ? (
                <>
                  {onRestore && (
                    <button type="button" role="menuitem" onClick={() => { setMenuOpen(false); onRestore(); }}>
                      {t("transactions.restore")}
                    </button>
                  )}
                  {onDeletePermanently && (
                    <button
                      type="button"
                      role="menuitem"
                      className="menu-danger"
                      onClick={() => { setMenuOpen(false); onDeletePermanently(); }}
                    >
                      {t("transactions.deletePermanent")}
                    </button>
                  )}
                  {transaction.has_receipt && (
                    <button
                      type="button"
                      role="menuitem"
                      onClick={() => {
                        setMenuOpen(false);
                        window.open(receiptUrl(transaction.id), "_blank", "noopener");
                      }}
                    >
                      {t("transactions.viewReceipt")}
                    </button>
                  )}
                </>
              ) : (
                <>
                  {onEdit && (
                    <button type="button" role="menuitem" onClick={() => { setMenuOpen(false); onEdit(); }}>
                      {t("transactions.edit")}
                    </button>
                  )}
                  {onUploadReceipt && (
                    <>
                      <button type="button" role="menuitem" onClick={() => fileRef.current?.click()}>
                        {t("transactions.uploadReceipt")}
                      </button>
                      {transaction.has_receipt && (
                        <button
                          type="button"
                          role="menuitem"
                          onClick={() => {
                            setMenuOpen(false);
                            window.open(receiptUrl(transaction.id), "_blank", "noopener");
                          }}
                        >
                          {t("transactions.viewReceipt")}
                        </button>
                      )}
                      <input
                        ref={fileRef}
                        type="file"
                        accept="image/*,application/pdf"
                        hidden
                        onChange={(event) => {
                          const file = event.target.files?.[0];
                          if (file) onUploadReceipt(file);
                          event.target.value = "";
                          setMenuOpen(false);
                        }}
                      />
                    </>
                  )}
                </>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
