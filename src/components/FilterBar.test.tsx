import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { FilterBar } from "./FilterBar";

describe("FilterBar", () => {
  it("shows humanized status labels rather than raw enum values", () => {
    render(<FilterBar />);
    expect(screen.getByText("Needs review")).toBeDefined();
    expect(screen.getByText("Waiting on me")).toBeDefined();
    expect(screen.queryByText("needsReview")).toBeNull();
    expect(screen.queryByText("waitingOnMe")).toBeNull();
  });

  it("gives the search box an accessible name", () => {
    render(<FilterBar />);
    expect(screen.getByRole("textbox", { name: "Search sessions" })).toBeDefined();
  });
});
