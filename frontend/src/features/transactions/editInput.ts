//! 编辑交易弹窗的提交载荷构建：对比表单与交易原值，只输出有变化的字段。
//!
//! 纯函数、与 React 解耦，便于单元测试。拆分提交语义（与后端 PATCH 契约一致）：
//! - 拆分有变化才携带 `splits` 键；非空数组整体替换，空数组清除拆分；
//! - 拆分未变化时请求体不出现 `splits` 键（后端视为「不动拆分」）。

import { toLocalDateTimeValue } from "../../lib";
import type { Transaction } from "../../types";

/** 拆分行编辑态（与后端 SplitInput 对齐；note 为空串时后端视为无备注）。 */
export interface SplitRow {
  category_id: number;
  amount: string;
  note: string;
}

/** PATCH /api/transactions/{id} 的请求体（仅包含有变化的字段）。 */
export interface TransactionEditInput {
  note?: string;
  occurred_at?: string;
  category_id?: number;
  amount?: string;
  account_id?: number;
  settled_amount?: string;
  tag_names?: string[];
  payee_name?: string;
  /** 拆分有变化时才携带：非空数组整体替换；空数组清除。 */
  splits?: SplitRow[];
}

export interface BuildEditInputParams {
  transaction: Transaction;
  note: string;
  /** datetime-local 输入值（本地时区、无时区后缀）。 */
  occurredAt: string;
  categoryId: number;
  amount: string;
  settledAmount: string;
  accountId: number;
  tagNames: string[];
  payeeName: string;
  /** 外币交易（交易币种 != 账户币种）时结算额需显式提交。 */
  foreign: boolean;
  splits: SplitRow[];
  originalSplits: SplitRow[];
}

/** 对比表单与交易原值，构建仅含变化字段的 PATCH 请求体。 */
export function buildEditInput(params: BuildEditInputParams): TransactionEditInput {
  const { transaction, splits, originalSplits } = params;
  const input: TransactionEditInput = {};
  if (params.note !== transaction.note) input.note = params.note;
  if (params.occurredAt !== toLocalDateTimeValue(transaction.occurred_at)) {
    input.occurred_at = new Date(params.occurredAt).toISOString();
  }
  if (params.categoryId !== (transaction.category_id ?? 0)) input.category_id = params.categoryId;
  if (params.accountId !== transaction.account_id) input.account_id = params.accountId;
  if (params.amount !== transaction.amount) input.amount = params.amount;
  // 外币交易：金额或结算额任一变化都一并提交结算额，保证后端校验通过；
  // 同币种交易结算额恒等于金额，由后端按新金额推导。
  if (
    params.foreign &&
    (params.amount !== transaction.amount || params.settledAmount !== transaction.settled_amount)
  ) {
    input.settled_amount = params.settledAmount;
  }
  if (params.tagNames.join(",") !== transaction.tags.join(",")) {
    input.tag_names = params.tagNames;
  }
  // 商户：值变化才提交；空串表示清除。
  if (params.payeeName.trim() !== (transaction.payee_name ?? "")) {
    input.payee_name = params.payeeName.trim();
  }
  // 拆分：有变化才携带（非空替换 / 空数组清除）；未变化不出现该键。
  if (JSON.stringify(splits) !== JSON.stringify(originalSplits)) {
    input.splits = splits;
  }
  return input;
}
