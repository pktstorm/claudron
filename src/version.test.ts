import { describe, it, expect } from "vitest";
import { isOlder } from "./version";

describe("isOlder", () => {
  it("compares numerically, not as strings", () => {
    expect(isOlder("2.1.99", "2.1.220")).toBe(true);
    expect(isOlder("2.1.220", "2.1.99")).toBe(false);
  });

  it("treats equal versions as not older", () => {
    expect(isOlder("2.1.220", "2.1.220")).toBe(false);
  });

  it("handles differing segment counts", () => {
    expect(isOlder("2.1", "2.1.1")).toBe(true);
    expect(isOlder("2.2", "2.1.9")).toBe(false);
  });

  it("treats an unparseable segment as zero", () => {
    expect(isOlder("2.1.beta", "2.1.1")).toBe(true);
  });

  it("parses a suffixed segment as zero, matching Rust", () => {
    // Number.parseInt would prefix-parse "220-beta" to 220; Rust's u32 parse
    // gives 0. They must agree, or backend and frontend disagree on "older".
    expect(isOlder("2.1.220-beta", "2.1.1")).toBe(true);
    expect(isOlder("2.1.220-beta", "2.1.0")).toBe(false);
    expect(isOlder("2.1.0", "2.1.220-beta")).toBe(false);
  });
});
