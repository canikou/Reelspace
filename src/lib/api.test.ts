import { describe, expect, it } from "vitest";
import { normalizeAppError, ReelspaceError } from "./api";

describe("normalizeAppError", () => {
  it("preserves an already normalized error", () => {
    const original = new ReelspaceError({
      code: "databaseBusy",
      message: "The library is busy",
      retryable: true
    });
    expect(normalizeAppError(original)).toBe(original);
  });

  it("accepts only complete structured IPC errors", () => {
    const normalized = normalizeAppError({
      code: "sourceUnavailable",
      message: "The source is unavailable",
      retryable: true,
      ignored: "not propagated"
    });
    expect(normalized).toMatchObject({
      name: "ReelspaceError",
      code: "sourceUnavailable",
      message: "The source is unavailable",
      retryable: true
    });
  });

  it.each([
    undefined,
    null,
    "plain string",
    42,
    true,
    [],
    {},
    { code: "sourceUnavailable" },
    { code: "sourceUnavailable", message: "missing retryable" },
    { code: "sourceUnavailable", message: 7, retryable: true },
    { code: "sourceUnavailable", message: "wrong retryable", retryable: "yes" },
    { code: "inventedCode", message: "not trusted", retryable: true },
    new Error("private implementation detail")
  ])("converts malformed input %# to a safe internal error", (value) => {
    const normalized = normalizeAppError(value);
    expect(normalized).toMatchObject({
      code: "internal",
      message: "An unexpected Reelspace error occurred",
      retryable: false
    });
    expect(normalized.message).not.toContain("private implementation detail");
  });
});
