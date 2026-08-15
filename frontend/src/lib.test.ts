import { describe, expect, it } from "vitest";
import {
  availableCurrencies,
  buildDonutGradient,
  categoryVisual,
  formatMoney,
  healthScore
} from "./lib";
import type { Account, MonthlySummary, Transaction } from "./types";

describe("formatMoney", () => {
  it("formats CNY with symbol and two decimals", () => {
    expect(formatMoney("1234.5", "CNY")).toBe("¥1,234.50");
  });

  it("formats USD with symbol", () => {
    expect(formatMoney("99", "USD")).toBe("$99.00");
  });

  it("formats negative values with a minus sign", () => {
    expect(formatMoney("-100", "CNY")).toBe("-¥100.00");
  });

  it("uses compact notation with zero decimals for large values", () => {
    expect(formatMoney("12345678", "CNY", true)).toBe("¥1234.6万");
  });

  it("falls back to raw text for non-finite values", () => {
    expect(formatMoney("abc", "CNY")).toBe("abc CNY");
  });
});

describe("availableCurrencies", () => {
  it("returns common currencies plus account currencies, deduplicated", () => {
    const accounts = [
      { currency: "CNY" },
      { currency: "THB" }
    ] as Account[];
    const result = availableCurrencies(accounts);
    expect(result).toContain("CNY");
    expect(result).toContain("THB");
    expect(result.indexOf("CNY")).toBe(result.lastIndexOf("CNY"));
    expect(result[0]).toBe("CNY");
  });

  it("includes transaction currencies", () => {
    const transactions = [{ currency: "EUR" }] as Transaction[];
    expect(availableCurrencies([], transactions)).toContain("EUR");
  });
});

describe("categoryVisual", () => {
  it("returns the preset style for known categories", () => {
    const visual = categoryVisual("餐饮");
    expect(visual.color).toBe("#d0784e");
  });

  it("generates a stable color for custom categories", () => {
    const first = categoryVisual("猫咪用品");
    const second = categoryVisual("猫咪用品");
    expect(first.color).toBe(second.color);
    expect(first.color).toMatch(/^#[0-9a-f]{6}$/i);
  });
});

describe("healthScore", () => {
  it("returns 100 when there is no income and no expense", () => {
    expect(healthScore({ total_income: "0", total_expense: "0" } as MonthlySummary)).toBe(100);
  });

  it("returns 0 when there is no income but there is expense", () => {
    expect(healthScore({ total_income: "0", total_expense: "50" } as MonthlySummary)).toBe(0);
  });

  it("computes retention ratio clamped to 0-100", () => {
    expect(healthScore({ total_income: "1000", total_expense: "200" } as MonthlySummary)).toBe(80);
    expect(healthScore({ total_income: "100", total_expense: "300" } as MonthlySummary)).toBe(0);
  });
});

describe("buildDonutGradient", () => {
  it("returns the fallback when there are no expenses", () => {
    expect(buildDonutGradient({ expenses_by_category: [] } as unknown as MonthlySummary)).toBe(
      "var(--border) 0 100%"
    );
  });

  it("builds color stops in percentage order", () => {
    const summary = {
      expenses_by_category: [
        { category_name: "餐饮", percentage: "70" },
        { category_name: "交通", percentage: "30" }
      ]
    } as unknown as MonthlySummary;
    const gradient = buildDonutGradient(summary);
    expect(gradient).toBe("#d0784e 0% 70%, #5077a5 70% 100%");
  });
});
