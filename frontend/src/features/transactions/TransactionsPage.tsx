//! 交易页：搜索/筛选/导出/导入与流水表格。
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Download, LoaderCircle, Plus, Search, Tags, Upload } from "lucide-react";
import { PageTitle } from "../../components/PageTitle";
import { EmptyState } from "../../components/EmptyState";
import { useConversionRates } from "../../components/accountDisplay";
import { TransactionRow } from "./TransactionRow";
import { exportTransactions, loadTagSummary } from "../../api";
import { formatMoney } from "../../lib";
import type { AppData, Tag, TagSummary, Transaction, TransactionKind } from "../../types";

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
  exportMonth
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
}) {
  const [search, setSearch] = useState("");
  const [kind, setKind] = useState<"all" | TransactionKind>("all");
  const [tagFilter, setTagFilter] = useState<string[]>([]);
  const [tagSummary, setTagSummary] = useState<TagSummary | null>(null);
  const [tagSummaryError, setTagSummaryError] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [exportError, setExportError] = useState<string | null>(null);
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
      </div>
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
      <article className="panel transaction-table">
        <div className="table-header"><span>{t("transactions.colTransaction")}</span><span>{t("transactions.colAccount")}</span><span>{t("transactions.colDate")}</span><span>{t("transactions.colAmount")}</span><span /><span /></div>
        {filtered.map((transaction) => (
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
        ))}
        {filtered.length === 0 && <EmptyState title={t("transactions.notFoundTitle")} detail={t("transactions.notFoundDetail")} />}
      </article>
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
