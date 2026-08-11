import { describe, it, expect, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { FilterBar } from "./FilterBar";
import { useFilters } from "../store/filters";

describe("FilterBar", () => {
  // The filter store is a module-level zustand singleton, so state survives
  // between tests in one file. Without this reset a test that selects a filter
  // silently changes what every later test renders.
  beforeEach(() => {
    useFilters.setState({ search: "", liveness: null, status: null });
  });

  it("marks the chip for the active filter as pressed", () => {
    useFilters.setState({ liveness: "idle" });
    render(<FilterBar />);

    expect(screen.getByRole("button", { name: "Idle" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByRole("button", { name: "Interrupted" }).getAttribute("aria-pressed")).toBe(
      "false",
    );
  });

  it("clears the filter when a pressed chip is clicked again", () => {
    useFilters.setState({ liveness: "idle" });
    render(<FilterBar />);

    fireEvent.click(screen.getByRole("button", { name: "Idle" }));

    expect(useFilters.getState().liveness).toBeNull();
  });

  it("selects the filter when an unpressed chip is clicked", () => {
    render(<FilterBar />);

    fireEvent.click(screen.getByRole("button", { name: "Idle" }));

    expect(useFilters.getState().liveness).toBe("idle");
  });

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
