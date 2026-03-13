import { describe, expect, it } from "vitest";
import { buildLogExport, formatTimestamp } from "./time";

describe("formatTimestamp", () => {
  it("returns Never for null", () => {
    expect(formatTimestamp(null)).toBe("Never");
  });

  it("renders a date string for non-zero values", () => {
    const nonZero = formatTimestamp(1);
    expect(nonZero.length).toBeGreaterThan(0);
  });
});

describe("buildLogExport", () => {
  it("builds stable ISO log lines", () => {
    const text = buildLogExport([
      { timestamp_ms: 1_700_000_000_000, level: "info", message: "hello" },
      { timestamp_ms: 1_700_000_001_000, level: "warn", message: "world" },
    ]);

    expect(text).toContain("[INFO] hello");
    expect(text).toContain("[WARN] world");
  });
});
