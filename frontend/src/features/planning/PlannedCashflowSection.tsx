//! 计划收支：把自动入账的周期交易与仅提醒的固定账单放在同一个使用场景中。
import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { ArrowDownLeft, ArrowUpRight, BellRing, Pause, Pencil, Play, Plus, Power, Trash2 } from "lucide-react";
import { createBill, deleteBill, getBills, getRecurringPreview, updateBill, type BillInput } from "../../api";
import { EmptyState } from "../../components/EmptyState";
import { ModalShell } from "../../components/ModalShell";
import { formatDate, formatMoney } from "../../lib";
import type { Account, Bill, Category, RecurrenceFrequency, RecurringRule } from "../../types";

const emptyBill = (): BillInput => ({ name: "", account_id: 0, category_id: 0, amount: "", due_day: 1, active: true, note: "" });
const frequencyKey: Record<RecurrenceFrequency, string> = {
  weekly: "common.weekly",
  monthly: "common.monthly",
  quarterly: "common.quarterly",
  yearly: "common.yearly"
};

export function PlannedCashflowSection({
  rules,
  accounts,
  categories,
  onCreateRecurring,
  onDeleteRecurring,
  onEditRecurring,
  onToggleRecurringPaused
}: {
  rules: RecurringRule[];
  accounts: Account[];
  categories: Category[];
  onCreateRecurring: () => void;
  onDeleteRecurring: (id: number) => void;
  onEditRecurring: (rule: RecurringRule) => void;
  onToggleRecurringPaused: (rule: RecurringRule) => void;
}) {
  const [bills, setBills] = useState<Bill[]>([]);
  const [previews, setPreviews] = useState<Record<number, string[]>>({});
  const [editingBill, setEditingBill] = useState<Bill | null>(null);
  const [creatingBill, setCreatingBill] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const accountMap = useMemo(() => new Map(accounts.map((item) => [item.id, item])), [accounts]);
  const categoryMap = useMemo(() => new Map(categories.map((item) => [item.id, item])), [categories]);
  const { t } = useTranslation();

  const loadBills = useCallback(async () => {
    try {
      setBills(await getBills());
      setError(null);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("planning.loadFailed"));
    }
  }, [t]);

  useEffect(() => { void loadBills(); }, [loadBills]);
  useEffect(() => {
    let cancelled = false;
    Promise.all(rules.map(async (rule) => [rule.id, (await getRecurringPreview(rule.id)).map((item) => item.due_at)] as const))
      .then((items) => { if (!cancelled) setPreviews(Object.fromEntries(items)); })
      .catch((reason) => { if (!cancelled) setError(reason instanceof Error ? reason.message : t("planning.previewFailed")); });
    return () => { cancelled = true; };
  }, [rules, t]);

  const removeBill = async (id: number) => {
    if (!window.confirm(t("planning.confirmDelete"))) return;
    try {
      await deleteBill(id);
      await loadBills();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("planning.deleteFailed"));
    }
  };

  const toggleBill = async (bill: Bill) => {
    try {
      await updateBill(bill.id, { name: bill.name, account_id: bill.account_id, category_id: bill.category_id, amount: bill.amount, due_day: bill.due_day, active: !bill.active, note: bill.note });
      await loadBills();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("planning.saveFailed"));
    }
  };

  return (
    <section className="section-block account-group">
      <div className="section-heading compact-heading">
        <div><span>{t("planning.eyebrow")}</span><h2>{t("planning.title")}</h2></div>
        <div className="section-actions">
          <button className="text-button" onClick={onCreateRecurring}><Plus size={16} /> {t("planning.autoEntry")}</button>
          <button className="text-button" onClick={() => setCreatingBill(true)}><Plus size={16} /> {t("planning.reminder")}</button>
        </div>
      </div>
      <p className="section-hint">{t("planning.hint")}</p>
      {error && <div className="inline-error" role="alert">{error}</div>}
      <div className="account-grid">
        {rules.map((rule) => {
          const account = accountMap.get(rule.account_id);
          const category = categoryMap.get(rule.category_id);
          const expense = rule.kind === "expense";
          const Icon = expense ? ArrowUpRight : ArrowDownLeft;
          return (
            <article className="account-detail-card planned-card" key={`rule-${rule.id}`}>
              <span className={`large-account-icon ${expense ? "tone-1" : "tone-2"}`}><Icon size={23} /></span>
              <div className="account-detail-copy">
                <span className="planned-badge automatic">{t("planning.automatic")}</span>
                <h3>{rule.note || category?.name || t("planning.recurringFallback")}{rule.paused_at && <em>{t("planning.paused")}</em>}</h3>
                <span>
                  {category?.name ?? t("planning.uncategorized")} · {t(frequencyKey[rule.frequency])} · {t("planning.next", { date: formatDate(rule.next_due_at) })} · {account?.name ?? t("common.unknownAccount")}
                  {!rule.paused_at && previews[rule.id]?.length ? ` · ${previews[rule.id].map(formatDate).join(" / ")}` : ""}
                </span>
              </div>
              <strong className={expense ? "expense-text" : "income-text"}>{expense ? "−" : "+"}{formatMoney(rule.amount, account?.currency ?? "CNY")}</strong>
              <div className="account-card-actions">
                <button className="row-action" onClick={() => onEditRecurring(rule)} aria-label={t("planning.editRecurring")}><Pencil size={16} /></button>
                <button className="row-action" onClick={() => onToggleRecurringPaused(rule)} aria-label={t(rule.paused_at ? "planning.resumeRecurring" : "planning.pauseRecurring")}>{rule.paused_at ? <Play size={16} /> : <Pause size={16} />}</button>
                <button className="row-action danger" onClick={() => onDeleteRecurring(rule.id)} aria-label={t("planning.deleteRecurring")}><Trash2 size={16} /></button>
              </div>
            </article>
          );
        })}
        {bills.map((bill) => {
          const account = accountMap.get(bill.account_id);
          return (
            <article className="account-detail-card planned-card" key={`bill-${bill.id}`}>
              <span className="large-account-icon tone-1"><BellRing size={23} /></span>
              <div className="account-detail-copy">
                <span className="planned-badge reminder">{t("planning.reminder")}</span>
                <h3>{bill.name}{!bill.active && <em>{t("planning.disabled")}</em>}</h3>
                <span>{t("planning.monthlyDue", { day: bill.due_day })} · {categoryMap.get(bill.category_id)?.name ?? t("planning.uncategorized")} · {account?.name ?? t("common.unknownAccount")}</span>
              </div>
              <strong>{formatMoney(bill.amount, account?.currency ?? "CNY")}</strong>
              <div className="account-card-actions">
                <button className="row-action" onClick={() => setEditingBill(bill)} aria-label={t("planning.editBill")}><Pencil size={16} /></button>
                <button className="row-action" onClick={() => void toggleBill(bill)} aria-label={t(bill.active ? "planning.disableBill" : "planning.enableBill")}><Power size={16} /></button>
                <button className="row-action danger" onClick={() => void removeBill(bill.id)} aria-label={t("planning.deleteBill")}><Trash2 size={16} /></button>
              </div>
            </article>
          );
        })}
        {rules.length === 0 && bills.length === 0 && <EmptyState title={t("planning.emptyTitle")} detail={t("planning.emptyDetail")} />}
      </div>
      {(creatingBill || editingBill) && <BillModal bill={editingBill} accounts={accounts} categories={categories} onClose={() => { setCreatingBill(false); setEditingBill(null); }} onSaved={() => void loadBills()} />}
    </section>
  );
}

function BillModal({ bill, accounts, categories, onClose, onSaved }: { bill: Bill | null; accounts: Account[]; categories: Category[]; onClose: () => void; onSaved: () => void }) {
  const [draft, setDraft] = useState<BillInput>(() => bill ? { name: bill.name, account_id: bill.account_id, category_id: bill.category_id, amount: bill.amount, due_day: bill.due_day, active: bill.active, note: bill.note } : emptyBill());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const save = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      if (bill) await updateBill(bill.id, draft);
      else await createBill(draft);
      onSaved();
      onClose();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("planning.saveFailed"));
    } finally {
      setBusy(false);
    }
  };
  return (
    <ModalShell eyebrow="REMINDER" title={t(bill ? "planning.editReminder" : "planning.addReminder")} onClose={onClose}>
      <form className="entry-form" onSubmit={(event) => void save(event)}>
        <div className="form-grid">
          <label><span>{t("planning.name")}</span><input required maxLength={80} value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></label>
          <label><span>{t("planning.dueDay")}</span><input required type="number" min="1" max="31" value={draft.due_day} onChange={(event) => setDraft({ ...draft, due_day: Number(event.target.value) })} /></label>
          <label><span>{t("planning.paymentAccount")}</span><select required value={draft.account_id || ""} onChange={(event) => setDraft({ ...draft, account_id: Number(event.target.value) })}><option value="" disabled>{t("common.selectAccount")}</option>{accounts.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label>
          <label><span>{t("planning.expenseCategory")}</span><select required value={draft.category_id || ""} onChange={(event) => setDraft({ ...draft, category_id: Number(event.target.value) })}><option value="" disabled>{t("planning.selectCategory")}</option>{categories.filter((item) => item.kind === "expense").map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label>
          <label><span>{t("planning.expectedAmount")}</span><input required type="number" min="0.01" step="0.01" value={draft.amount} onChange={(event) => setDraft({ ...draft, amount: event.target.value })} /></label>
          <label><span>{t("common.note")}</span><input value={draft.note} onChange={(event) => setDraft({ ...draft, note: event.target.value })} /></label>
        </div>
        {error && <div className="inline-error" role="alert">{error}</div>}
        <div className="modal-actions"><button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button><button className="primary-button" disabled={busy}>{busy ? t("common.saving") : t("common.save")}</button></div>
      </form>
    </ModalShell>
  );
}
