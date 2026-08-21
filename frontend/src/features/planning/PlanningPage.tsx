//! 自动化与规划中心：管理导入模板、交易规则、账单及储蓄目标。
import { useCallback, useEffect, useMemo, useState, type FormEvent, type ReactNode } from "react";
import { Plus, RotateCcw, Trash2 } from "lucide-react";
import {
  applyTransactionRule,
  createBill,
  createImportProfile,
  createSavingsGoal,
  createTransactionRule,
  deleteBill,
  deleteImportProfile,
  deleteSavingsGoal,
  deleteTransactionRule,
  getBills,
  getImportProfiles,
  getSavingsGoals,
  getTransactionRules,
  updateBill,
  updateImportProfile,
  updateSavingsGoal,
  updateTransactionRule,
  type BillInput,
  type ImportProfileInput,
  type SavingsGoalInput,
  type TransactionRuleInput
} from "../../api";
import { EmptyState } from "../../components/EmptyState";
import { formatMoney } from "../../lib";
import type { Account, Bill, Category, ImportProfile, SavingsGoal, TransactionRule } from "../../types";

const profileBlank = (): ImportProfileInput => ({ name: "", format: "auto", account_id: null, category_id: null, currency: null });
const billBlank = (): BillInput => ({ name: "", account_id: 0, category_id: 0, amount: "", due_day: 1, active: true, note: "" });
const goalBlank = (): SavingsGoalInput => ({ name: "", account_id: null, target_amount: "", current_amount: "0", target_date: null });
const ruleBlank = (): TransactionRuleInput => ({ name: "", enabled: true, priority: 0, description_contains: null, account_id: null, kind: "expense", min_amount: null, max_amount: null, category_id: null, payee_name: null, tag_names: [] });
const toOptional = (value: string) => value.trim() || null;

export function PlanningPage({ accounts, categories }: { accounts: Account[]; categories: Category[] }) {
  const [profiles, setProfiles] = useState<ImportProfile[]>([]);
  const [bills, setBills] = useState<Bill[]>([]);
  const [goals, setGoals] = useState<SavingsGoal[]>([]);
  const [rules, setRules] = useState<TransactionRule[]>([]);
  const [profile, setProfile] = useState(profileBlank);
  const [bill, setBill] = useState(billBlank);
  const [goal, setGoal] = useState(goalBlank);
  const [rule, setRule] = useState(ruleBlank);
  const [editing, setEditing] = useState<{ type: "profile" | "bill" | "goal" | "rule"; id: number } | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const accountMap = useMemo(() => new Map(accounts.map((item) => [item.id, item])), [accounts]);
  const categoryMap = useMemo(() => new Map(categories.map((item) => [item.id, item])), [categories]);
  const expenseCategories = categories.filter((item) => item.kind === "expense");

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      const [p, b, g, r] = await Promise.all([getImportProfiles(), getBills(), getSavingsGoals(), getTransactionRules()]);
      setProfiles(p); setBills(b); setGoals(g); setRules(r);
      setMessage(null);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "无法读取自动化与规划数据");
    } finally { setLoading(false); }
  }, []);
  useEffect(() => { void reload(); }, [reload]);

  const submit = async (event: FormEvent, type: "profile" | "bill" | "goal" | "rule") => {
    event.preventDefault(); setBusy(true); setMessage(null);
    try {
      const currentId = editing?.type === type ? editing.id : null;
      if (type === "profile") currentId ? await updateImportProfile(currentId, profile) : await createImportProfile(profile);
      if (type === "bill") currentId ? await updateBill(currentId, bill) : await createBill(bill);
      if (type === "goal") currentId ? await updateSavingsGoal(currentId, goal) : await createSavingsGoal(goal);
      if (type === "rule") currentId ? await updateTransactionRule(currentId, rule) : await createTransactionRule(rule);
      setEditing(null);
      if (type === "profile") setProfile(profileBlank());
      if (type === "bill") setBill(billBlank());
      if (type === "goal") setGoal(goalBlank());
      if (type === "rule") setRule(ruleBlank());
      await reload(); setMessage("已保存");
    } catch (error) { setMessage(error instanceof Error ? error.message : "保存失败"); }
    finally { setBusy(false); }
  };
  const remove = async (type: "profile" | "bill" | "goal" | "rule", id: number) => {
    if (!window.confirm("确认删除这项设置？")) return;
    setBusy(true); setMessage(null);
    try {
      if (type === "profile") await deleteImportProfile(id);
      if (type === "bill") await deleteBill(id);
      if (type === "goal") await deleteSavingsGoal(id);
      if (type === "rule") await deleteTransactionRule(id);
      await reload(); setMessage("已删除");
    } catch (error) { setMessage(error instanceof Error ? error.message : "删除失败"); }
    finally { setBusy(false); }
  };
  const cancel = (type: "profile" | "bill" | "goal" | "rule") => {
    setEditing(null);
    if (type === "profile") setProfile(profileBlank());
    if (type === "bill") setBill(billBlank());
    if (type === "goal") setGoal(goalBlank());
    if (type === "rule") setRule(ruleBlank());
  };

  return <div className="page-stack planning-page">
    <div className="page-heading"><div><span>PLAN & AUTOMATE</span><h1>自动化与规划</h1><p>保存导入习惯、管理固定账单、储蓄目标和自动分类规则。</p></div><button className="secondary-button" onClick={() => void reload()} disabled={loading || busy}><RotateCcw size={16} /> 刷新</button></div>
    {message && <div className="inline-error">{message}</div>}
    <section className="section-block"><div className="section-heading"><div><span>IMPORT</span><h2>导入模板</h2></div></div>
      <form className="entry-form compact-form" onSubmit={(event) => void submit(event, "profile")}><div className="form-grid">
        <label><span>模板名称</span><input required value={profile.name} onChange={(e) => setProfile({ ...profile, name: e.target.value })} placeholder="例如：招商银行信用卡" /></label>
        <label><span>文件格式</span><select value={profile.format} onChange={(e) => setProfile({ ...profile, format: e.target.value as ImportProfileInput["format"] })}><option value="auto">自动识别</option><option value="csv">CSV</option><option value="qif">QIF</option><option value="ofx">OFX</option></select></label>
        <label><span>默认账户</span><select value={profile.account_id ?? ""} onChange={(e) => setProfile({ ...profile, account_id: e.target.value ? Number(e.target.value) : null })}><option value="">不预设</option>{accounts.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label>
        <label><span>默认分类</span><select value={profile.category_id ?? ""} onChange={(e) => setProfile({ ...profile, category_id: e.target.value ? Number(e.target.value) : null })}><option value="">不预设</option>{categories.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label>
        <label><span>默认币种</span><input value={profile.currency ?? ""} onChange={(e) => setProfile({ ...profile, currency: toOptional(e.target.value) })} placeholder="CNY" /></label>
      </div><FormActions editing={editing?.type === "profile"} busy={busy} onCancel={() => cancel("profile")} /></form>
      <List empty="尚未保存导入模板。" loading={loading}>{profiles.map((item) => <article className="planning-row" key={item.id}><div><strong>{item.name}</strong><span>{item.format.toUpperCase()} · {item.account_id ? accountMap.get(item.account_id)?.name : "未设账户"} · {item.category_id ? categoryMap.get(item.category_id)?.name : "未设分类"}{item.currency ? ` · ${item.currency}` : ""}</span></div><RowActions onEdit={() => { setEditing({ type: "profile", id: item.id }); setProfile({ name: item.name, format: item.format, account_id: item.account_id, category_id: item.category_id, currency: item.currency }); }} onDelete={() => void remove("profile", item.id)} /></article>)}</List>
    </section>
    <section className="section-block"><div className="section-heading"><div><span>BILLS</span><h2>账单中心</h2></div></div>
      <form className="entry-form compact-form" onSubmit={(event) => void submit(event, "bill")}><div className="form-grid"><label><span>账单名称</span><input required value={bill.name} onChange={(e) => setBill({ ...bill, name: e.target.value })} placeholder="例如：宽带" /></label><label><span>每月到期日</span><input required type="number" min="1" max="31" value={bill.due_day} onChange={(e) => setBill({ ...bill, due_day: Number(e.target.value) })} /></label><label><span>付款账户</span><select required value={bill.account_id || ""} onChange={(e) => setBill({ ...bill, account_id: Number(e.target.value) })}><option value="" disabled>请选择</option>{accounts.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label><label><span>支出分类</span><select required value={bill.category_id || ""} onChange={(e) => setBill({ ...bill, category_id: Number(e.target.value) })}><option value="" disabled>请选择</option>{expenseCategories.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label><label><span>预计金额</span><input required type="number" min="0.01" step="0.01" value={bill.amount} onChange={(e) => setBill({ ...bill, amount: e.target.value })} /></label><label><span>备注</span><input value={bill.note} onChange={(e) => setBill({ ...bill, note: e.target.value })} /></label><label className="checkbox-label"><input type="checkbox" checked={bill.active} onChange={(e) => setBill({ ...bill, active: e.target.checked })} /> 启用提醒</label></div><FormActions editing={editing?.type === "bill"} busy={busy} onCancel={() => cancel("bill")} /></form>
      <List empty="还没有固定账单。" loading={loading}>{bills.map((item) => <article className="planning-row" key={item.id}><div><strong>{item.name} {!item.active && <em>已停用</em>}</strong><span>每月 {item.due_day} 日 · {formatMoney(item.amount, accountMap.get(item.account_id)?.currency ?? "CNY")} · {accountMap.get(item.account_id)?.name} · {categoryMap.get(item.category_id)?.name}</span></div><RowActions onEdit={() => { setEditing({ type: "bill", id: item.id }); setBill({ name: item.name, account_id: item.account_id, category_id: item.category_id, amount: item.amount, due_day: item.due_day, active: item.active, note: item.note }); }} onDelete={() => void remove("bill", item.id)} /></article>)}</List>
    </section>
    <section className="section-block"><div className="section-heading"><div><span>GOALS</span><h2>储蓄目标</h2></div></div>
      <form className="entry-form compact-form" onSubmit={(event) => void submit(event, "goal")}><div className="form-grid"><label><span>目标名称</span><input required value={goal.name} onChange={(e) => setGoal({ ...goal, name: e.target.value })} placeholder="例如：旅行基金" /></label><label><span>目标金额</span><input required type="number" min="0.01" step="0.01" value={goal.target_amount} onChange={(e) => setGoal({ ...goal, target_amount: e.target.value })} /></label><label><span>当前已存</span><input required type="number" min="0" step="0.01" value={goal.current_amount} onChange={(e) => setGoal({ ...goal, current_amount: e.target.value })} /></label><label><span>关联账户</span><select value={goal.account_id ?? ""} onChange={(e) => setGoal({ ...goal, account_id: e.target.value ? Number(e.target.value) : null })}><option value="">不关联</option>{accounts.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label><label><span>目标日期</span><input type="date" value={goal.target_date ?? ""} onChange={(e) => setGoal({ ...goal, target_date: toOptional(e.target.value) })} /></label></div><FormActions editing={editing?.type === "goal"} busy={busy} onCancel={() => cancel("goal")} /></form>
      <List empty="还没有储蓄目标。" loading={loading}>{goals.map((item) => { const progress = Math.min(100, Number(item.current_amount) / Number(item.target_amount) * 100 || 0); return <article className="planning-row goal-row" key={item.id}><div><strong>{item.name}</strong><span>{formatMoney(item.current_amount, accountMap.get(item.account_id ?? 0)?.currency ?? "CNY")} / {formatMoney(item.target_amount, accountMap.get(item.account_id ?? 0)?.currency ?? "CNY")} · {progress.toFixed(0)}%{item.target_date ? ` · ${item.target_date}` : ""}</span><div className="goal-progress"><i style={{ width: `${progress}%` }} /></div></div><RowActions onEdit={() => { setEditing({ type: "goal", id: item.id }); setGoal({ name: item.name, account_id: item.account_id, target_amount: item.target_amount, current_amount: item.current_amount, target_date: item.target_date }); }} onDelete={() => void remove("goal", item.id)} /></article>; })}</List>
    </section>
    <section className="section-block"><div className="section-heading"><div><span>RULES</span><h2>交易自动规则</h2></div></div>
      <form className="entry-form compact-form" onSubmit={(event) => void submit(event, "rule")}><div className="form-grid"><label><span>规则名称</span><input required value={rule.name} onChange={(e) => setRule({ ...rule, name: e.target.value })} placeholder="例如：滴滴自动归为通勤" /></label><label><span>描述包含</span><input value={rule.description_contains ?? ""} onChange={(e) => setRule({ ...rule, description_contains: toOptional(e.target.value) })} placeholder="滴滴" /></label><label><span>限定账户</span><select value={rule.account_id ?? ""} onChange={(e) => setRule({ ...rule, account_id: e.target.value ? Number(e.target.value) : null })}><option value="">不限</option>{accounts.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label><label><span>交易类型</span><select value={rule.kind ?? ""} onChange={(e) => setRule({ ...rule, kind: e.target.value ? e.target.value as "expense" | "income" : null })}><option value="">不限</option><option value="expense">支出</option><option value="income">收入</option></select></label><label><span>最低金额</span><input type="number" min="0" step="0.01" value={rule.min_amount ?? ""} onChange={(e) => setRule({ ...rule, min_amount: toOptional(e.target.value) })} /></label><label><span>最高金额</span><input type="number" min="0" step="0.01" value={rule.max_amount ?? ""} onChange={(e) => setRule({ ...rule, max_amount: toOptional(e.target.value) })} /></label><label><span>目标分类</span><select value={rule.category_id ?? ""} onChange={(e) => setRule({ ...rule, category_id: e.target.value ? Number(e.target.value) : null })}><option value="">不修改</option>{categories.filter((item) => !rule.kind || item.kind === rule.kind).map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label><label><span>设置商户</span><input value={rule.payee_name ?? ""} onChange={(e) => setRule({ ...rule, payee_name: toOptional(e.target.value) })} /></label><label><span>标签（逗号分隔）</span><input value={rule.tag_names.join(", ")} onChange={(e) => setRule({ ...rule, tag_names: e.target.value.split(",").map((x) => x.trim()).filter(Boolean) })} /></label><label><span>优先级（越小越先）</span><input type="number" value={rule.priority} onChange={(e) => setRule({ ...rule, priority: Number(e.target.value) })} /></label><label className="checkbox-label"><input type="checkbox" checked={rule.enabled} onChange={(e) => setRule({ ...rule, enabled: e.target.checked })} /> 启用规则</label></div><FormActions editing={editing?.type === "rule"} busy={busy} onCancel={() => cancel("rule")} /></form>
      <List empty="还没有自动规则。" loading={loading}>{rules.map((item) => <article className="planning-row" key={item.id}><div><strong>{item.name} {!item.enabled && <em>已停用</em>}</strong><span>优先级 {item.priority} · {item.description_contains ? `包含“${item.description_contains}”` : "所有交易"}{item.category_id ? ` → ${categoryMap.get(item.category_id)?.name}` : ""}{item.payee_name ? ` · 商户：${item.payee_name}` : ""}{item.tag_names.length ? ` · #${item.tag_names.join(" #")}` : ""}</span></div><div className="row-buttons"><button className="text-button" disabled={busy} onClick={() => void (async () => { setBusy(true); try { const result = await applyTransactionRule(item.id); setMessage(`已应用到 ${result.applied} 笔交易`); } catch (error) { setMessage(error instanceof Error ? error.message : "应用失败"); } finally { setBusy(false); } })()}>应用到历史</button><RowActions onEdit={() => { setEditing({ type: "rule", id: item.id }); setRule({ name: item.name, enabled: item.enabled, priority: item.priority, description_contains: item.description_contains, account_id: item.account_id, kind: item.kind as "expense" | "income" | null, min_amount: item.min_amount, max_amount: item.max_amount, category_id: item.category_id, payee_name: item.payee_name, tag_names: item.tag_names }); }} onDelete={() => void remove("rule", item.id)} /></div></article>)}</List>
    </section>
  </div>;
}

function FormActions({ editing, busy, onCancel }: { editing: boolean; busy: boolean; onCancel: () => void }) { return <div className="modal-actions"><button className="primary-button" disabled={busy}><Plus size={16} /> {editing ? "更新" : "新增"}</button>{editing && <button type="button" className="secondary-button" onClick={onCancel}>取消编辑</button>}</div>; }
function List({ children, empty, loading }: { children: ReactNode; empty: string; loading: boolean }) { const list = Array.isArray(children) ? children : [children]; return <div className="planning-list">{loading ? <p>正在加载…</p> : list.length ? list : <EmptyState title={empty} detail="创建后可随时在此维护。" />}</div>; }
function RowActions({ onEdit, onDelete }: { onEdit: () => void; onDelete: () => void }) { return <div className="row-buttons"><button className="text-button" onClick={onEdit}>编辑</button><button className="row-action danger" onClick={onDelete} aria-label="删除"><Trash2 size={16} /></button></div>; }
