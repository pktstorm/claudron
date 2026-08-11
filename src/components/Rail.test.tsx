import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { Rail } from "./Rail";

describe("Rail", () => {
  it("marks only the active view as the current page", () => {
    // `aria-current` is the affordance AND the test hook. Styling the active
    // item with a class alone would leave assistive technology unable to tell
    // which view is showing, and would leave this assertion reading class
    // strings instead of behaviour.
    render(<Rail active="dashboard" onSelect={() => {}} />);

    expect(screen.getByRole("button", { name: "Dashboard" }).getAttribute("aria-current")).toBe(
      "page",
    );
    expect(screen.getByRole("button", { name: "Sessions" }).getAttribute("aria-current")).toBeNull();
  });

  it("reports the view the user picked", () => {
    const onSelect = vi.fn();
    render(<Rail active="sessions" onSelect={onSelect} />);

    fireEvent.click(screen.getByRole("button", { name: "Dashboard" }));

    expect(onSelect.mock.calls).toEqual([["dashboard"]]);
  });
});
