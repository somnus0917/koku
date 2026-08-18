//! 新建账户弹窗。
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import type { AccountType } from "../../types";

export function AccountModal({
  currencies,
  onClose,
  onSubmit
}: {
  currencies: string[];
  onClose: () => void;
  onSubmit: (input: {
    name: string;
    account_type: AccountType;
    currency: string;
    opening_balance: string;
    credit_limit?: string;
    statement_day?: number;
    due_day?: number;
  }) => Promise<void>;
}) {
  const [name, setName] = useState("");
  const [type, setType] = useState<AccountType>("cash");
  const [currency, setCurrency] = useState("CNY");
  const [balance, setBalance] = useState("0");
  const [creditLimit, setCreditLimit] = useState("");
  const [statementDay, setStatementDay] = useState("");
  const [dueDay, setDueDay] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const submit = async (event: FormEvent) => {
    event.preventDefault(); setSubmitting(true); setError(null);
    try {
      await onSubmit({
        name,
        account_type: type,
        currency,
        opening_balance: balance,
        credit_limit: creditLimit.trim() ? creditLimit.trim() : undefined,
        statement_day: statementDay.trim() ? Number(statementDay.trim()) : undefined,
        due_day: dueDay.trim() ? Number(dueDay.trim()) : undefined
      });
    }
    catch (reason) { setError(reason instanceof Error ? reason.message : t("modals.transaction.saveFailed")); setSubmitting(false); }
  };
  return (
    <ModalShell eyebrow="NEW ACCOUNT" title={t("accounts.newAccount")} onClose={onClose}>
      <form className="entry-form" onSubmit={submit}>
        <div className="form-grid">
          <label className="span-two"><span>{t("modals.account.name")}</span><input autoFocus required value={name} onChange={(e) => setName(e.target.value)} placeholder={t("modals.account.namePlaceholder")} /></label>
          <label><span>{t("modals.editAccount.type")}</span><select value={type} onChange={(e) => setType(e.target.value as AccountType)}>
            <option value="cash">{t("accounts.type.cash")}</option>
            <option value="savings">{t("accounts.type.savings")}</option>
            <option value="stock">{t("accounts.type.stock")}</option>
            <option value="credit">{t("accounts.type.credit")}</option>
          </select></label>
          <label><span>{t("modals.account.currency")}</span>
            <select value={currency} onChange={(e) => setCurrency(e.target.value)}>
              {currencies.map((item) => <option key={item} value={item}>{item}</option>)}
            </select>
          </label>
          <label className="span-two"><span>{t("modals.account.openingBalance")}</span><input required step="0.01" inputMode="decimal" value={balance} onChange={(e) => setBalance(e.target.value)} /></label>
          {type === "credit" && (
            <>
              <label className="span-two"><span>{t("modals.account.creditLimit")}</span><input step="0.01" inputMode="decimal" value={creditLimit} onChange={(e) => setCreditLimit(e.target.value)} placeholder={t("modals.editAccount.creditLimitPlaceholder")} /></label>
              <label><span>{t("modals.editAccount.statementDay")}</span>
                <input type="number" min="1" max="31" step="1" value={statementDay} onChange={(e) => setStatementDay(e.target.value)} placeholder={t("modals.editAccount.statementDayPlaceholder")} />
              </label>
              <label><span>{t("modals.editAccount.dueDay")}</span>
                <input type="number" min="1" max="31" step="1" value={dueDay} onChange={(e) => setDueDay(e.target.value)} placeholder={t("modals.editAccount.dueDayPlaceholder")} />
              </label>
            </>
          )}
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions"><button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button><button className="primary-button" disabled={submitting}>{submitting && <LoaderCircle className="spin" size={17} />}{t("modals.account.submit")}</button></div>
      </form>
    </ModalShell>
  );
}
