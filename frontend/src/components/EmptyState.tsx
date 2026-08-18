//! 空状态占位。
import { ReceiptText } from "lucide-react";

export function EmptyState({ title, detail }: { title: string; detail: string }) {
  return <div className="empty-state"><span><ReceiptText size={20} /></span><div><strong>{title}</strong><p>{detail}</p></div></div>;
}
