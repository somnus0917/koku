//! 借款/出借 API：创建与还款。
import { request } from "./client";
import type { Loan, LoanType } from "../types";

export function createLoan(input: {
  loan_type: LoanType;
  counterparty: string;
  currency?: string;
  amount: string;
  account_id: number;
  note?: string;
  due_at?: string;
}): Promise<Loan> {
  return request("/api/loans", {
    method: "POST",
    body: JSON.stringify(input)
  });
}
export function repayLoan(
  loanId: number,
  input: {
    account_id: number;
    amount: string;
    currency?: string;
    settled_amount?: string;
    note?: string;
  }
): Promise<Loan> {
  return request(`/api/loans/${loanId}/repay`, {
    method: "POST",
    body: JSON.stringify(input)
  });
}
