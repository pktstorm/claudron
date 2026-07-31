import { describe, it, expect } from "vitest";
import { relativeAge } from "./time";

const NOW = 1_800_000_000_000; // fixed clock, ms

describe("relativeAge", () => {
  it("renders sub-minute as now", () => {
    expect(relativeAge(NOW / 1000 - 5, NOW)).toBe("now");
  });
  it("renders minutes, hours, and days", () => {
    expect(relativeAge(NOW / 1000 - 120, NOW)).toBe("2m");
    expect(relativeAge(NOW / 1000 - 7200, NOW)).toBe("2h");
    expect(relativeAge(NOW / 1000 - 172800, NOW)).toBe("2d");
  });
  it("renders an empty string for a zero timestamp", () => {
    expect(relativeAge(0, NOW)).toBe("");
  });
  it("never renders a negative age", () => {
    expect(relativeAge(NOW / 1000 + 999, NOW)).toBe("now");
  });
});
