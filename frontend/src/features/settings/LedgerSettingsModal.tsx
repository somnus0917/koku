//! 账本设置：将全局交易规则留在设置入口，而不是独立的专业化工作台。
import { useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Check, LoaderCircle, Plus, Trash2, WandSparkles } from "lucide-react";
import {
  applyTransactionRule,
  createTransactionRule,
  deleteTransactionRule,
  getTransactionRules,
  previewTransactionRule,
  updateTransactionRule,
  type TransactionRuleInput
} from "../../api";
import { ModalShell } from "../../components/ModalShell";
import type { Account, Category, TransactionRule, TransactionRulePreview } from "../../types";

const blank = (): TransactionRuleInput => ({ name: "", enabled: true, priority: 0, description_contains: null, account_id: null, kind: "expense", min_amount: null, max_amount: null, category_id: null, payee_name: null, tag_names: [] });
const optional = (value: string) => value.trim() || null;

export function LedgerSettingsModal({ accounts, categories, onClose }: { accounts: Account[]; categories: Category[]; onClose: () => void }) {
  const [rules, setRules] = useState<TransactionRule[]>([]);
  const [draft, setDraft] = useState(blank);
  const [editing, setEditing] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [previewRule, setPreviewRule] = useState<TransactionRule | null>(null);
  const [previews, setPreviews] = useState<TransactionRulePreview[]>([]);
  const [selectedPreviewIds, setSelectedPreviewIds] = useState<number[]>([]);
  const categoryMap = useMemo(() => new Map(categories.map((item) => [item.id, item.name])), [categories]);
  const { t } = useTranslation();

  const load = useCallback(async () => {
    try {
      setRules(await getTransactionRules());
    } catch (error) {
      setMessage(error instanceof Error ? error.message : t("ledgerSettings.loadFailed"));
    }
  }, [t]);
  useEffect(() => { void load(); }, [load]);

  const reset = () => { setEditing(null); setDraft(blank()); };
  const save = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setMessage(null);
    try {
      if (editing && editing > 0) await updateTransactionRule(editing, draft);
      else await createTransactionRule(draft);
      await load();
      reset();
      setMessage(t("ledgerSettings.saved"));
    } catch (error) {
      setMessage(error instanceof Error ? error.message : t("ledgerSettings.saveFailed"));
    } finally {
      setBusy(false);
    }
  };
  const edit = (item: TransactionRule) => {
    setEditing(item.id);
    setDraft({ name: item.name, enabled: item.enabled, priority: item.priority, description_contains: item.description_contains, account_id: item.account_id, kind: item.kind as "expense" | "income" | null, min_amount: item.min_amount, max_amount: item.max_amount, category_id: item.category_id, payee_name: item.payee_name, tag_names: item.tag_names });
  };
  const remove = async (id: number) => {
    if (!window.confirm(t("ledgerSettings.confirmDelete"))) return;
    setBusy(true);
    try {
      await deleteTransactionRule(id);
      await load();
      if (editing === id) reset();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : t("ledgerSettings.deleteFailed"));
    } finally {
      setBusy(false);
    }
  };
  const openPreview = async (rule: TransactionRule) => {
    setBusy(true);
    setMessage(null);
    try {
      const result = await previewTransactionRule(rule.id);
      setPreviewRule(rule);
      setPreviews(result);
      setSelectedPreviewIds(result.map((item) => item.transaction_id));
    } catch (error) {
      setMessage(error instanceof Error ? error.message : t("ledgerSettings.previewFailed"));
    } finally {
      setBusy(false);
    }
  };
  const applyPreview = async () => {
    if (!previewRule) return;
    setBusy(true);
    try {
      const result = await applyTransactionRule(previewRule.id, selectedPreviewIds);
      setMessage(t("ledgerSettings.applied", { count: result.applied }));
      setPreviewRule(null);
      setPreviews([]);
      setSelectedPreviewIds([]);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : t("ledgerSettings.applyFailed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <ModalShell eyebrow="LEDGER SETTINGS" title={t("ledgerSettings.title")} onClose={onClose}>
      <div className="settings-intro"><WandSparkles size={17} /><p>{t("ledgerSettings.intro")}</p></div>
      {message && <div className="totp-notice" role="status"><Check size={14} /> {message}</div>}
      <div className="settings-rule-list">
        {rules.map((item) => (
          <article key={item.id} className="settings-rule-row">
            <div>
              <strong>{item.name}{!item.enabled && <em>{t("ledgerSettings.disabled")}</em>}</strong>
              <span>{item.description_contains ? t("ledgerSettings.descriptionContains", { value: item.description_contains }) : t("ledgerSettings.allTransactions")}{item.category_id ? t("ledgerSettings.categorySuffix", { value: categoryMap.get(item.category_id) }) : ""}{item.payee_name ? t("ledgerSettings.payeeSuffix", { value: item.payee_name }) : ""}</span>
            </div>
            <div>
              <button className="text-button" disabled={busy || !item.enabled} onClick={() => void openPreview(item)}>{t("ledgerSettings.preview")}</button>
              <button className="text-button" disabled={busy} onClick={() => edit(item)}>{t("common.edit")}</button>
              <button className="row-action danger" disabled={busy} onClick={() => void remove(item.id)} aria-label={t("ledgerSettings.deleteRule")}><Trash2 size={16} /></button>
            </div>
          </article>
        ))}
      </div>
      {previewRule && (
        <section className="rule-preview">
          <header><div><small>RULE PREVIEW</small><strong>{t("ledgerSettings.confirmBackfill", { name: previewRule.name })}</strong></div><button className="row-action" type="button" onClick={() => setPreviewRule(null)} aria-label={t("common.close")}>×</button></header>
          <p>{t("ledgerSettings.previewHelp")}</p>
          {previews.length === 0 ? <span className="rule-preview-empty">{t("ledgerSettings.noChanges")}</span> : <>
            <div className="rule-preview-list">
              {previews.map((item) => <label key={item.transaction_id}><input type="checkbox" checked={selectedPreviewIds.includes(item.transaction_id)} onChange={(event) => setSelectedPreviewIds((ids) => event.target.checked ? [...ids, item.transaction_id] : ids.filter((id) => id !== item.transaction_id))} /><span><strong>{item.note || t("ledgerSettings.noNote")}</strong><small>{item.occurred_at.slice(0, 10)} · {item.amount} {item.currency}</small></span><em>{item.current_category_id ? categoryMap.get(item.current_category_id) ?? t("ledgerSettings.originalCategory") : t("planning.uncategorized")} → {item.suggested_category_id ? categoryMap.get(item.suggested_category_id) ?? t("ledgerSettings.newCategory") : t("planning.uncategorized")}</em></label>)}
            </div>
            <div className="modal-actions"><button type="button" className="secondary-button" onClick={() => setPreviewRule(null)}>{t("common.cancel")}</button><button type="button" className="primary-button" disabled={busy || selectedPreviewIds.length === 0} onClick={() => void applyPreview()}>{busy && <LoaderCircle className="spin" size={16} />}{t("ledgerSettings.applyCount", { count: selectedPreviewIds.length })}</button></div>
          </>}
        </section>
      )}
      {!editing && <button className="text-button settings-add" onClick={() => setEditing(-1)}><Plus size={16} /> {t("ledgerSettings.newRule")}</button>}
      {editing !== null && (
        <form className="entry-form settings-rule-form" onSubmit={(event) => void save(event)}>
          <div className="form-grid">
            <label><span>{t("ledgerSettings.ruleName")}</span><input required maxLength={80} value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} placeholder={t("ledgerSettings.ruleNamePlaceholder")} /></label>
            <label><span>{t("ledgerSettings.descriptionField")}</span><input value={draft.description_contains ?? ""} onChange={(event) => setDraft({ ...draft, description_contains: optional(event.target.value) })} placeholder={t("ledgerSettings.descriptionPlaceholder")} /></label>
            <label><span>{t("ledgerSettings.accountScope")}</span><select value={draft.account_id ?? ""} onChange={(event) => setDraft({ ...draft, account_id: event.target.value ? Number(event.target.value) : null })}><option value="">{t("ledgerSettings.allAccounts")}</option>{accounts.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label>
            <label><span>{t("ledgerSettings.transactionType")}</span><select value={draft.kind ?? ""} onChange={(event) => setDraft({ ...draft, kind: event.target.value ? event.target.value as "expense" | "income" : null })}><option value="">{t("ledgerSettings.anyType")}</option><option value="expense">{t("common.expense")}</option><option value="income">{t("common.income")}</option></select></label>
            <label><span>{t("ledgerSettings.applyCategory")}</span><select value={draft.category_id ?? ""} onChange={(event) => setDraft({ ...draft, category_id: event.target.value ? Number(event.target.value) : null })}><option value="">{t("ledgerSettings.noCategoryChange")}</option>{categories.filter((item) => !draft.kind || item.kind === draft.kind).map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label>
            <label><span>{t("ledgerSettings.payeeName")}</span><input value={draft.payee_name ?? ""} onChange={(event) => setDraft({ ...draft, payee_name: optional(event.target.value) })} /></label>
            <label><span>{t("ledgerSettings.tags")}</span><input value={draft.tag_names.join(", ")} onChange={(event) => setDraft({ ...draft, tag_names: event.target.value.split(",").map((item) => item.trim()).filter(Boolean) })} /></label>
            <label className="checkbox-label"><input type="checkbox" checked={draft.enabled} onChange={(event) => setDraft({ ...draft, enabled: event.target.checked })} /> {t("ledgerSettings.enableRule")}</label>
          </div>
          <div className="modal-actions"><button className="secondary-button" type="button" onClick={reset}>{t("common.cancel")}</button><button className="primary-button" disabled={busy}>{busy && <LoaderCircle className="spin" size={16} />}{t(editing > 0 ? "ledgerSettings.saveChanges" : "ledgerSettings.saveRule")}</button></div>
        </form>
      )}
      <div className="modal-actions"><button type="button" className="secondary-button" onClick={onClose}>{t("common.done")}</button></div>
    </ModalShell>
  );
}
