//! 交易页：搜索/筛选/导出/导入与流水表格。
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { BookmarkPlus, Download, List, LoaderCircle, Plus, Rows3, Search, Tags, Trash2, Upload } from "lucide-react";
import { PageTitle } from "../../components/PageTitle";
import { EmptyState } from "../../components/EmptyState";
import { useConversionRates } from "../../components/accountDisplay";
import { TransactionRow } from "./TransactionRow";
import { exportTransactions, loadTagSummary } from "../../api";
import { formatMoney } from "../../lib";
import type { AppData, Tag, TagSummary, Transaction, TransactionKind } from "../../types";

type TransactionViewMode = "table" | "timeline";
type SavedTransactionView = { id: string; name: string; search: string; kind: "all" | TransactionKind; tags: string[]; mode: TransactionViewMode };
const SAVED_VIEWS_KEY = "koku.transaction.saved-views.v1";

function readSavedViews(): SavedTransactionView[] {
  try {
    const value: unknown = JSON.parse(window.localStorage.getItem(SAVED_VIEWS_KEY) ?? "[]");
    if (!Array.isArray(value)) return [];
    return value.filter((item): item is SavedTransactionView => Boolean(item) && typeof item === "object" && typeof item.id === "string" && typeof item.name === "string" && typeof item.search === "string" && (item.kind === "all" || typeof item.kind === "string") && Array.isArray(item.tags) && (item.mode === "table" || item.mode === "timeline"));
  } catch {
    return [];
  }
}

function dateGroup(value: string): string {
  const date = new Date(value);
  return new Intl.DateTimeFormat(undefined, { month: "long", day: "numeric", weekday: "short" }).format(date);
}

/**
 * 标签多选下拉：可同时勾选多个标签（AND 语义），选中后标签筛选生效。
 */
function TagMultiSelect({
  tags,
  selected,
  onChange
}: {
  tags: Tag[];
  selected: string[];
  onChange: (next: string[]) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);
  const toggle = (name: string) => {
    onChange(selected.includes(name) ? selected.filter((item) => item !== name) : [...selected, name]);
  };
  const { t } = useTranslation();
  return (
    <div className="tag-multiselect" ref={ref}>
      <button
        type="button"
        className={`tag-filter-button ${selected.length > 0 ? "active" : ""}`}
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        aria-haspopup="listbox"
      >
        <Tags size={14} />
        {selected.length === 0 ? t("transactions.tagFilter") : selected.join(" + ")}
      </button>
      {open && (
        <div className="tag-multiselect-menu" role="listbox" aria-multiselectable="true">
          {tags.map((tag) => {
            const checked = selected.includes(tag.name);
            return (
              <label key={tag.id} className="tag-multiselect-option">
                <input type="checkbox" checked={checked} onChange={() => toggle(tag.name)} />
                {tag.name}
              </label>
            );
          })}
          {selected.length > 0 && (
            <button type="button" className="tag-multiselect-clear" onClick={() => onChange([])}>
              {t("transactions.clearTagFilter")}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
export function TransactionsPage({
  data,
  onAdd,
  onImport,
  onVoid,
  onRestore,
  onDeletePermanently,
  onMarkReimbursable,
  onUnmarkReimbursable,
  onReimburse,
  onEdit,
  onUploadReceipt,
  onLoadMore,
  loadingMore = false,
  hasMore = false,
  exportYear,
  exportMonth,
  onFilterChange
}: {
  data: AppData;
  onAdd: () => void;
  /** 打开批量导入弹窗。 */
  onImport: () => void;
  onVoid: (transaction: Transaction) => void;
  onRestore: (transaction: Transaction) => void;
  onDeletePermanently: (transaction: Transaction) => void;
  onMarkReimbursable: (transaction: Transaction) => void;
  onUnmarkReimbursable: (transaction: Transaction) => void;
  onReimburse: (transaction: Transaction) => void;
  onEdit: (transaction: Transaction) => void;
  onUploadReceipt: (transaction: Transaction, file: File) => void;
  onLoadMore?: () => void;
  loadingMore?: boolean;
  hasMore?: boolean;
  exportYear?: number;
  exportMonth?: number;
  onFilterChange?: (filters: { search: string; kind: string; tags: string[] }) => void;
}) {
  const [search, setSearch] = useState("");
  const [kind, setKind] = useState<"all" | TransactionKind>("all");
  const [tagFilter, setTagFilter] = useState<string[]>([]);
  const [tagSummary, setTagSummary] = useState<TagSummary | null>(null);
  const [tagSummaryError, setTagSummaryError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [exportError, setExportError] = useState<string | null>(null);
  const [mode, setMode] = useState<TransactionViewMode>(() => window.localStorage.getItem("koku.transaction.view-mode") === "timeline" ? "timeline" : "table");
  const [savedViews, setSavedViews] = useState<SavedTransactionView[]>(readSavedViews);
  const { t } = useTranslation();
  const handleExport = async () => {
    setExporting(true);
    setExportError(null);
    try {
      await exportTransactions(exportYear, exportMonth);
    } catch (reason) {
      setExportError(reason instanceof Error ? reason.message : t("transactions.exportLoadFailed"));
    } finally {
      setExporting(false);
    }
  };
  const accountsById = useMemo(() => new Map(data.accounts.map((item) => [item.id, item])), [data.accounts]);
  const categoriesById = useMemo(() => new Map(data.categories.map((item) => [item.id, item])), [data.categories]);
  const display = data.monthly.currency;
  const txCurrencies = useMemo(
    () => [...new Set(data.transactions.map((item) => item.currency))],
    [data.transactions]
  );
  const rates = useConversionRates(txCurrencies, display);
  const tagFilterKey = tagFilter.join(",");
  const persistSavedViews = (next: SavedTransactionView[]) => {
    setSavedViews(next);
    window.localStorage.setItem(SAVED_VIEWS_KEY, JSON.stringify(next));
  };
  const saveCurrentView = () => {
    const name = window.prompt(t("transactions.savedViews.namePrompt"));
    if (!name?.trim()) return;
    persistSavedViews([...savedViews, { id: crypto.randomUUID(), name: name.trim().slice(0, 40), search, kind, tags: tagFilter, mode }]);
  };
  const selectSavedView = (view: SavedTransactionView) => {
    setSearch(view.search);
    setKind(view.kind);
    setTagFilter(view.tags);
    setMode(view.mode);
  };
  const changeMode = (next: TransactionViewMode) => {
    setMode(next);
    window.localStorage.setItem("koku.transaction.view-mode", next);
  };
  useEffect(() => {
    const timer = window.setTimeout(() => onFilterChange?.({ search, kind, tags: tagFilter }), 250);
    return () => window.clearTimeout(timer);
  }, [search, kind, tagFilterKey, onFilterChange]);
  // 选中标签时拉取对应汇总（月视图按当前月，全部月份视图按全部历史）。
  useEffect(() => {
    if (tagFilter.length === 0) {
      setTagSummary(null);
      setTagSummaryError(null);
      return;
    }
    let cancelled = false;
    setTagSummaryError(null);
    loadTagSummary(tagFilter, display, exportYear, exportMonth)
      .then((summary) => {
        if (!cancelled) setTagSummary(summary);
      })
      .catch((reason) => {
        if (!cancelled) {
          setTagSummary(null);
          setTagSummaryError(reason instanceof Error ? reason.message : t("transactions.tagSummaryLoadFailed"));
        }
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tagFilterKey, display, exportYear, exportMonth]);
  const filtered = data.transactions.filter((item) => {
    const category = item.category_id ? categoriesById.get(item.category_id)?.name ?? "" : "转账";
    const account = accountsById.get(item.account_id)?.name ?? "";
    const matchesSearch = `${item.note} ${item.payee_name ?? ""} ${category} ${account}`.toLowerCase().includes(search.toLowerCase());
    const matchesTag = tagFilter.length === 0 || tagFilter.every((name) => item.tags.includes(name));
    return matchesSearch && matchesTag && (kind === "all" || item.kind === kind);
  });
  const timelineGroups = filtered.reduce<Array<{ day: string; items: Transaction[] }>>((groups, item) => {
    const day = item.occurred_at.slice(0, 10);
    const current = groups.at(-1);
    if (current?.day === day) current.items.push(item);
    else groups.push({ day, items: [item] });
    return groups;
  }, []);
  const row = (transaction: Transaction) => (
    <TransactionRow
      key={transaction.id}
      transaction={transaction}
      account={accountsById.get(transaction.account_id)}
      target={transaction.to_account_id ? accountsById.get(transaction.to_account_id) : undefined}
      category={transaction.category_id ? categoriesById.get(transaction.category_id) : undefined}
      display={display}
      rates={rates}
      onVoid={() => onVoid(transaction)}
      onRestore={() => onRestore(transaction)}
      onDeletePermanently={() => onDeletePermanently(transaction)}
      onMarkReimbursable={() => onMarkReimbursable(transaction)}
      onUnmarkReimbursable={() => onUnmarkReimbursable(transaction)}
      onReimburse={() => onReimburse(transaction)}
      onEdit={() => onEdit(transaction)}
      onUploadReceipt={(file) => onUploadReceipt(transaction, file)}
    />
  );
  return (
    <div className="page page-enter">
      <PageTitle
        eyebrow="TRANSACTIONS"
        title={t("transactions.title")}
        actions={<button className="primary-button" onClick={onAdd}><Plus size={18} /> {t("common.quickAdd")}</button>}
      />
      <div className="transaction-toolbar">
        <label className="search-box"><Search size={18} /><input value={search} onChange={(e) => setSearch(e.target.value)} placeholder={t("transactions.searchPlaceholder")} /></label>
        <div className="segmented-filter">
          {(["all", "expense", "income", "transfer", "loan"] as const).map((item) => (
            <button key={item} className={kind === item ? "active" : ""} onClick={() => setKind(item)}>
              {t(`transactions.kind.${item}`)}
            </button>
          ))}
        </div>
        {data.tags.length > 0 && (
          <TagMultiSelect
            tags={data.tags}
            selected={tagFilter}
            onChange={setTagFilter}
          />
        )}
        <button
          type="button"
          className="text-button export-button"
          onClick={() => void handleExport()}
          disabled={exporting}
          title={exportYear !== undefined && exportMonth !== undefined ? t("transactions.exportTitle", { year: exportYear, month: exportMonth }) : t("transactions.exportAll")}
        >
          {exporting ? <LoaderCircle className="spin" size={16} /> : <Download size={16} />}
          {exporting ? t("transactions.exporting") : t("transactions.exportCsv")}
        </button>
        <button
          type="button"
          className="text-button import-button"
          onClick={onImport}
          title={t("transactions.importTitle")}
        >
          <Upload size={16} /> {t("transactions.import")}
        </button>
        <div className="transaction-view-actions">
          <div className="view-mode-toggle" aria-label={t("transactions.viewMode.label")}>
            <button type="button" className={mode === "table" ? "active" : ""} onClick={() => changeMode("table")} title={t("transactions.viewMode.table")}><List size={16} /></button>
            <button type="button" className={mode === "timeline" ? "active" : ""} onClick={() => changeMode("timeline")} title={t("transactions.viewMode.timeline")}><Rows3 size={16} /></button>
          </div>
          <button type="button" className="text-button save-view-button" onClick={saveCurrentView}><BookmarkPlus size={16} /> {t("transactions.savedViews.save")}</button>
        </div>
      </div>
      {savedViews.length > 0 && <div className="saved-views" aria-label={t("transactions.savedViews.label")}><span>{t("transactions.savedViews.label")}</span>{savedViews.map((view) => <div className="saved-view-chip" key={view.id}><button type="button" onClick={() => selectSavedView(view)}>{view.name}</button><button type="button" onClick={() => persistSavedViews(savedViews.filter((item) => item.id !== view.id))} aria-label={t("transactions.savedViews.remove", { name: view.name })}><Trash2 size={12} /></button></div>)}</div>}
      {tagFilter.length > 0 && (
        <section className="tag-summary" aria-label={t("transactions.tagSummaryAria")}>
          {tagSummaryError ? (
            <span className="inline-error">{t("transactions.tagSummaryError")}{tagSummaryError}</span>
          ) : tagSummary ? (
            <>
              <div className="tag-summary-total">
                <span className="tag-summary-label">
                  {t("transactions.tagSummary.tags", { tags: tagSummary.tags.join(" + ") })}
                  {tagSummary.year ? t("transactions.tagSummary.period", { year: tagSummary.year, month: tagSummary.month }) : t("transactions.tagSummary.allHistory")}
                  {t("transactions.tagSummary.total")}
                </span>
                <strong>{t("common.expense")} {formatMoney(tagSummary.total_expense, tagSummary.currency)}</strong>
                <span>{t("common.income")} {formatMoney(tagSummary.total_income, tagSummary.currency)}</span>
                <span className={Number(tagSummary.retained) >= 0 ? "positive" : "negative"}>
                  {t("common.net")} {formatMoney(tagSummary.retained, tagSummary.currency)}
                </span>
              </div>
              {tagSummary.expense_destinations.length > 0 && (
                <div className="tag-summary-breakdown">
                  {tagSummary.expense_destinations.map((item) => (
                    <span key={item.category_id} className="tag-summary-item">
                      {item.category_name} {formatMoney(item.amount, tagSummary.currency)}
                      <em>{item.percentage}%</em>
                    </span>
                  ))}
                </div>
              )}
            </>
          ) : (
            <span className="inline-error">{t("transactions.tagSummaryLoading")}</span>
          )}
        </section>
      )}
      {exportError && <div className="inline-error">{t("transactions.exportFailed")}{exportError}</div>}
      {mode === "table" ? <article className="panel transaction-table">
        <div className="table-header"><span>{t("transactions.colTransaction")}</span><span>{t("transactions.colAccount")}</span><span>{t("transactions.colDate")}</span><span>{t("transactions.colAmount")}</span><span /><span /></div>
        {filtered.map(row)}
        {filtered.length === 0 && <EmptyState title={t("transactions.notFoundTitle")} detail={t("transactions.notFoundDetail")} />}
      </article> : <div className="transaction-timeline panel">
        {timelineGroups.map((group) => <section key={group.day}><h2>{dateGroup(group.day)}</h2>{group.items.map(row)}</section>)}
        {filtered.length === 0 && <EmptyState title={t("transactions.notFoundTitle")} detail={t("transactions.notFoundDetail")} />}
      </div>}
      {hasMore && (
        <div className="load-more-row">
          <button
            type="button"
            className="text-button load-more-button"
            onClick={onLoadMore}
            disabled={loadingMore}
          >
            {loadingMore
              ? <><LoaderCircle className="spin" size={16} /> {t("common.loading")}</>
              : t("transactions.loadMore", { count: data.transactions.length })}
          </button>
        </div>
      )}
    </div>
  );
}
