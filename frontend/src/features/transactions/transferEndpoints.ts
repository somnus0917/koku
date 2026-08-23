//! 转账两端选择的纯状态转换，保证 UI 显示与提交校验使用同一对账户。

export type TransferEndpoints = { sourceId: number; targetId: number };

/** 选择新的转出账户时，若碰到当前转入账户，则自动交换两端。 */
export function selectTransferSource(
  { sourceId, targetId }: TransferEndpoints,
  nextSourceId: number
): TransferEndpoints {
  if (nextSourceId === targetId && sourceId !== targetId) {
    return { sourceId: nextSourceId, targetId: sourceId };
  }
  return { sourceId: nextSourceId, targetId };
}

/** 对称处理：选择新的转入账户时，同样不会留下相同的两个端点。 */
export function selectTransferTarget(
  { sourceId, targetId }: TransferEndpoints,
  nextTargetId: number
): TransferEndpoints {
  if (nextTargetId === sourceId && sourceId !== targetId) {
    return { sourceId: targetId, targetId: nextTargetId };
  }
  return { sourceId, targetId: nextTargetId };
}
