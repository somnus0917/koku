//! 编辑账户弹窗（改名/类型/币种/额度/账单日/还款日/余额调整）。
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import { formatMoney } from "../../lib";
import type { Account, AccountType } from "../../types";

export function EditAccountModal({
  account,
  currencies,
  onClose,
  onSubmit
}: {
  account: Account;
  currencies: string[];
  onClose: () => void;
  onSubmit: (input: {
    details: {
      name?: string;
      account_type?: AccountType;
      currency?: string;
      credit_limit?: string | null;
      statement_day?: number | null;
      due_day?: number | null;
    };
    adjustment?: string;
  }) => Promise<void>;
}) {
  const [name, setName] = useState(account.name);
  const [type, setType] = useState<AccountType>(account.account_type);
  const [currency, setCurrency] = useState(account.currency);
  const [creditLimit, setCreditLimit] = useState(account.credit_limit ?? "");
  const [statementDay, setStatementDay] = useState(account.statement_day ? String(account.statement_day) : "");
  const [dueDay, setDueDay] = useState(account.due_day ? String(account.due_day) : "");
  const [adjustment, setAdjustment] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const isCredit = type === "credit";
  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitting(true); setError(null);
    try {
      const details: {
        name?: string;
        account_type?: AccountType;
        currency?: string;
        credit_limit?: string | null;
        statement_day?: number | null;
        due_day?: number | null;
      } = {
        name: name.trim() !== account.name ? name.trim() : undefined,
        account_type: type !== account.account_type ? type : undefined,
        currency: currency !== account.currency ? currency : undefined
      };
      // 信用卡专属字段只在账户为 Credit 时提交（切换走非信用则不动旧值）。
      if (isCredit) {
        const limitChanged = creditLimit.trim() !== (account.credit_limit ?? "");
        if (limitChanged) {
          details.credit_limit = creditLimit.trim() ? creditLimit.trim() : null;
        }
        const statementChanged = (statementDay.trim() ? Number(statementDay.trim()) : null) !== (account.statement_day ?? null);
        if (statementChanged) {
          details.statement_day = statementDay.trim() ? Number(statementDay.trim()) : null;
        }
        const dueChanged = (dueDay.trim() ? Number(dueDay.trim()) : null) !== (account.due_day ?? null);
        if (dueChanged) {
          details.due_day = dueDay.trim() ? Number(dueDay.trim()) : null;
        }
      }
      await onSubmit({
        details,
        adjustment: adjustment ? adjustment : undefined
      });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("modals.transaction.saveFailed"));
      setSubmitting(false);
    }
  };
  return (
    <ModalShell eyebrow="EDIT ACCOUNT" title={t("accounts.editTitle")} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="deposit-info">
          <p>{t("modals.editAccount.info", { name: account.name, amount: formatMoney(account.balance, account.currency) })}</p>
        </div>
        <div className="form-grid">
          <label className="span-two"><span>{t("modals.editAccount.name")}</span><input value={name} onChange={(e) => setName(e.target.value)} /></label>
          <label><span>{t("modals.editAccount.type")}</span><select value={type} onChange={(e) => setType(e.target.value as AccountType)}>
            <option value="cash">{t("accounts.type.cash")}</option>
            <option value="savings">{t("accounts.type.savings")}</option>
            <option value="stock">{t("accounts.type.stock")}</option>
            <option value="credit">{t("accounts.type.credit")}</option>
          </select></label>
          <label><span>{t("modals.editAccount.currency")}</span><select value={currency} onChange={(e) => setCurrency(e.target.value)}>
            {currencies.map((item) => <option key={item} value={item}>{item}</option>)}
          </select></label>
          {isCredit && (
            <>
              <label><span>{t("modals.editAccount.creditLimit")}</span>
                <input step="0.01" inputMode="decimal" value={creditLimit} onChange={(e) => setCreditLimit(e.target.value)} placeholder={t("modals.editAccount.creditLimitPlaceholder")} />
              </label>
              <label><span>{t("modals.editAccount.statementDay")}</span>
                <input type="number" min="1" max="31" step="1" value={statementDay} onChange={(e) => setStatementDay(e.target.value)} placeholder={t("modals.editAccount.statementDayPlaceholder")} />
              </label>
              <label><span>{t("modals.editAccount.dueDay")}</span>
                <input type="number" min="1" max="31" step="1" value={dueDay} onChange={(e) => setDueDay(e.target.value)} placeholder={t("modals.editAccount.dueDayPlaceholder")} />
              </label>
            </>
          )}
          <label className="span-two"><span>{t("modals.editAccount.adjustment")}</span>
            <input step="0.01" inputMode="decimal" value={adjustment} onChange={(e) => setAdjustment(e.target.value)} placeholder="0.00" />
          </label>
        </div>
        <p className="category-delete-note">{t("modals.editAccount.adjustmentNote")}</p>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions">
          <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
          <button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}{t("common.save")}</button>
        </div>
      </form>
    </ModalShell>
  );
}
