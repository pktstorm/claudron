import { describe, it, expect } from "vitest";
import { scanFraction } from "./scan";

const p = (bytesDone: number, bytesTotal: number) => ({
  filesDone: 0,
  filesTotal: 0,
  bytesDone,
  bytesTotal,
});

describe("scanFraction", () => {
  it("is the ratio of bytes done to total", () => {
    expect(scanFraction(p(25, 100))).toBe(0.25);
  });

  it("treats an empty tree as complete rather than dividing by zero", () => {
    expect(scanFraction(p(0, 0))).toBe(1);
  });

  it("clamps above 1, because a live session grows its transcript mid-scan", () => {
    // bytesTotal is fixed at enumeration; a session writing during the scan
    // pushes bytesDone past it, and a bar over 100% looks broken.
    expect(scanFraction(p(500, 100))).toBe(1);
  });

  it("clamps below 0", () => {
    expect(scanFraction(p(-10, 100))).toBe(0);
  });
});
