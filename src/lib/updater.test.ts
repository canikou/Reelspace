import { describe, expect, it } from "vitest";
import { keepPendingUpdate } from "./updater";

describe("keepPendingUpdate", () => {
  const available = { phase: "available" as const, version: "1.2.3" };

  it("keeps a shown update when a background check finds nothing, errors, or starts", () => {
    expect(keepPendingUpdate(available, { phase: "idle" }, false)).toBe(available);
    expect(keepPendingUpdate(available, { phase: "checking" }, false)).toBe(available);
    const downloading = { phase: "downloading" as const, version: "1.2.3" };
    expect(keepPendingUpdate(downloading, { phase: "idle" }, false)).toBe(downloading);
  });

  it("lets a manual check replace the state", () => {
    expect(keepPendingUpdate(available, { phase: "idle" }, true)).toEqual({ phase: "idle" });
  });

  it("lets a background check replace a state with no pending update", () => {
    expect(keepPendingUpdate({ phase: "idle" }, available, false)).toBe(available);
  });
});
