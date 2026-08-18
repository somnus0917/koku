//! 批量导入交易弹窗：选择账单文件与目标账户，展示导入结果。
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { LoaderCircle, Upload } from "lucide-react";
import { ModalShell } from "../../components/ModalShell";
import { importTransactions } from "../../api";
import type { Account, Category, ImportResult } from "../../types";

/** 批量导入交易：选择账单文件与目标账户，导入后展示结果摘要（成功/重复/失败 + 问题行）。
 *  表单提交由本弹窗直接调用 API 以拿到 ImportResult 展示；「完成」时调用父级 onComplete
 *  （父级按 mutate 模式刷新并提示，不再重复导入）。 */
export function ImportModal({
  accounts,
  categories,
  onClose,
  onComplete
}: {
  accounts: Account[];
  categories: Category[];
  onClose: () => void;
  /** 导入已完成：仅刷新数据并提示，不再重复调用导入 API。 */
  onComplete: () => void;
}) {
  const [accountId, setAccountId] = useState("");
  const [format, setFormat] = useState<"auto" | "csv" | "qif" | "ofx">("auto");
  const [categoryId, setCategoryId] = useState("");
  const [currency, setCurrency] = useState("");
  const [file, setFile] = useState<File | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<ImportResult | null>(null);
  const { t } = useTranslation();

  const input = (): { format?: string; account_id: number; category_id?: number; currency?: string } => ({
    format: format === "auto" ? undefined : format,
    account_id: Number(accountId),
    category_id: categoryId ? Number(categoryId) : undefined,
    currency: currency.trim() || undefined
  });

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!file) return;
    setSubmitting(true); setError(null);
    try {
      setResult(await importTransactions(file, input()));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("modals.import.importFailed"));
      setSubmitting(false);
    }
  };

  const finish = () => {
    onComplete();
    onClose();
  };

  return (
    <ModalShell eyebrow="IMPORT" title={t("modals.import.title")} onClose={onClose}>
      {result ? (
        <div className="import-result">
          <div className="import-summary">
            <span className="import-count ok"><strong>{result.imported}</strong>{t("modals.import.imported", { count: result.imported })}</span>
            <span className="import-count skip"><strong>{result.skipped_duplicates}</strong>{t("modals.import.skipped", { count: result.skipped_duplicates })}</span>
            <span className={`import-count ${result.failed > 0 ? "bad" : ""}`}><strong>{result.failed}</strong>{t("modals.import.failed", { count: result.failed })}</span>
          </div>
          {result.issues.length > 0 && (
            <div className="import-issues" aria-label={t("modals.import.issuesAria")}>
              <div className="import-issues-head">{t("modals.import.issuesHead", { count: result.issues.length })}</div>
              {result.issues.map((issue, index) => (
                <div className="import-issue" key={index}>
                  <span>{t("modals.import.row", { line: issue.line })}</span>
                  <span>{issue.message}</span>
                </div>
              ))}
            </div>
          )}
          <p className="fx-hint">
            {t("modals.import.doneHint", { format: result.format.toUpperCase() })}
          </p>
          <div className="modal-actions">
            <button type="button" className="secondary-button" onClick={onClose}>{t("common.close")}</button>
            <button type="button" className="primary-button" onClick={finish}>{t("modals.category.done")}</button>
          </div>
        </div>
      ) : (
        <form className="entry-form" onSubmit={submit}>
          <div className="deposit-info">
            <p>{t("modals.import.intro")}</p>
          </div>
          <div className="form-grid">
            <label><span>{t("modals.import.account")}</span>
              <select required value={accountId} onChange={(e) => setAccountId(e.target.value)}>
                <option value="" disabled>{t("common.selectAccount")}</option>
                {accounts.map((account) => (
                  <option key={account.id} value={account.id}>{account.name}（{account.currency}）</option>
                ))}
              </select>
            </label>
            <label><span>{t("modals.import.format")}</span>
              <select value={format} onChange={(e) => setFormat(e.target.value as "auto" | "csv" | "qif" | "ofx")}>
                <option value="auto">{t("modals.import.auto")}</option>
                <option value="csv">CSV</option>
                <option value="qif">QIF</option>
                <option value="ofx">OFX</option>
              </select>
            </label>
            <label className="span-two"><span>{t("modals.import.defaultCategory")}</span>
              <select value={categoryId} onChange={(e) => setCategoryId(e.target.value)}>
                <option value="">{t("modals.import.noCategory")}</option>
                {categories.map((category) => (
                  <option key={category.id} value={category.id}>
                    {t(category.kind === "income" ? "modals.import.incomePrefix" : "modals.import.expensePrefix")}{category.name}
                  </option>
                ))}
              </select>
            </label>
            <label><span>{t("modals.import.defaultCurrency")}</span>
              <input value={currency} onChange={(e) => setCurrency(e.target.value)} placeholder={t("modals.import.currencyPlaceholder")} />
            </label>
            <label className="span-two"><span>{t("modals.import.file")}</span>
              <input
                required
                type="file"
                accept=".csv,.qif,.ofx"
                onChange={(e) => setFile(e.target.files?.[0] ?? null)}
              />
            </label>
          </div>
          {error && <div className="form-error">{error}</div>}
          <div className="modal-actions">
            <button type="button" className="secondary-button" onClick={onClose}>{t("common.cancel")}</button>
            <button className="primary-button" disabled={submitting || !file || !accountId}>
              {submitting ? <LoaderCircle className="spin" size={17} /> : <Upload size={16} />}
              {submitting ? t("modals.import.importing") : t("modals.import.start")}
            </button>
          </div>
        </form>
      )}
    </ModalShell>
  );
}
