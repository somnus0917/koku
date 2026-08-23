import { describe, expect, it } from "vitest";
import { selectTransferSource, selectTransferTarget } from "./transferEndpoints";

describe("transfer endpoints", () => {
  it("swaps the target when the selected source is the current target", () => {
    expect(selectTransferSource({ sourceId: 2, targetId: 1 }, 1)).toEqual({ sourceId: 1, targetId: 2 });
  });

  it("keeps both endpoints unchanged when the source remains distinct", () => {
    expect(selectTransferSource({ sourceId: 2, targetId: 1 }, 3)).toEqual({ sourceId: 3, targetId: 1 });
  });

  it("handles target selection symmetrically", () => {
    expect(selectTransferTarget({ sourceId: 2, targetId: 1 }, 2)).toEqual({ sourceId: 1, targetId: 2 });
  });
});
