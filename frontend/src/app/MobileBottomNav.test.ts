import { describe, expect, it } from "vitest";
import { MOBILE_NAV_ITEMS } from "./MobileBottomNav";

describe("MOBILE_NAV_ITEMS", () => {
  it("keeps the bottom bar to four high-frequency views", () => {
    expect(MOBILE_NAV_ITEMS.map((item) => item.id)).toEqual([
      "dashboard",
      "accounts",
      "transactions",
      "insights"
    ]);
  });
});
