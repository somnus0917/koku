//! 分类管理弹窗：新增与删除分类。
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle, X } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import { CategoryAvatar } from "../../components/avatar";
import type { Category, CategoryKind } from "../../types";

export function CategoryModal({ categories, onClose, onSubmit, onDelete }: { categories: Category[]; onClose: () => void; onSubmit: (input: { name: string; kind: CategoryKind }) => Promise<void>; onDelete: (category: Category) => Promise<void> }) {
  const [name, setName] = useState("");
  const [kind, setKind] = useState<CategoryKind>("expense");
  const [submitting, setSubmitting] = useState(false);
  const [deletingId, setDeletingId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { t } = useTranslation();
  const submit = async (event: FormEvent) => {
    event.preventDefault(); setSubmitting(true); setError(null);
    try { await onSubmit({ name, kind }); }
    catch (reason) { setError(reason instanceof Error ? reason.message : t("modals.transaction.saveFailed")); setSubmitting(false); }
  };
  const remove = async (category: Category) => {
    if (!window.confirm(t("modals.category.confirmDelete", { name: category.name }))) return;
    setDeletingId(category.id); setError(null);
    try { await onDelete(category); }
    catch (reason) { setError(reason instanceof Error ? reason.message : t("modals.category.deleteFailed")); }
    finally { setDeletingId(null); }
  };
  return (
    <ModalShell eyebrow="CATEGORIES" title={t("modals.category.title")} onClose={onClose}>
      <div className="category-library">
        {([
          { kind: "expense" as const, label: t("modals.category.expenseGroup") },
          { kind: "income" as const, label: t("modals.category.incomeGroup") }
        ]).map((group) => {
          const items = categories.filter((item) => item.kind === group.kind);
          return (
            <section key={group.kind}>
              <header><strong>{group.label}</strong><small>{t("common.countItems", { count: items.length })}</small></header>
              <div className="category-chip-list">
                {items.map((item) => <span key={item.id} className={item.kind}><CategoryAvatar name={item.name} size="tiny" /><span>{item.name}</span><button type="button" onClick={() => void remove(item)} disabled={deletingId !== null} aria-label={t("modals.category.removeAria", { name: item.name })}>{deletingId === item.id ? <LoaderCircle className="spin" size={11} /> : <X size={11} />}</button></span>)}
              </div>
            </section>
          );
        })}
      </div>
      <form className="entry-form category-form" onSubmit={submit}>
        <div className="form-grid">
          <label><span>{t("modals.category.kind")}</span><select value={kind} onChange={(e) => setKind(e.target.value as CategoryKind)}><option value="expense">{t("common.expense")}</option><option value="income">{t("common.income")}</option></select></label>
          <label><span>{t("modals.category.name")}</span><input autoFocus required value={name} onChange={(e) => setName(e.target.value)} placeholder={t("modals.category.namePlaceholder")} /></label>
        </div>
        {error && <div className="form-error">{error}</div>}
        <div className="modal-actions"><button type="button" className="secondary-button" onClick={onClose}>{t("modals.category.done")}</button><button className="primary-button" disabled={submitting || !name}>{submitting && <LoaderCircle className="spin" size={17} />}{t("modals.category.add")}</button></div>
      </form>
    </ModalShell>
  );
}
