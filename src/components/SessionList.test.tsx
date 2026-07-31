import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { SessionList } from "./SessionList";
import type { Session } from "../types";

const mk = (over: Partial<Session>): Session => ({
  sessionId: "s1",
  aiTitle: "Fix the parser",
  lastPrompt: "make it work",
  gitBranch: "main",
  cwd: "/code/repo",
  projectLabel: "repo",
  version: "2.1.220",
  lastActivity: 100,
  liveness: "idle",
  annotation: { notes: "", status: null, displayName: null },
  ...over,
});

describe("SessionList", () => {
  it("renders one row per session", () => {
    render(
      <SessionList
        sessions={[mk({ sessionId: "a" }), mk({ sessionId: "b", aiTitle: "Other work" })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline={null}
      />,
    );
    expect(screen.getByText("Fix the parser")).toBeDefined();
    expect(screen.getByText("Other work")).toBeDefined();
  });

  it("prefers the user's display name over the AI title", () => {
    render(
      <SessionList
        sessions={[mk({ annotation: { notes: "", status: null, displayName: "My name" } })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline={null}
      />,
    );
    expect(screen.getByText("My name")).toBeDefined();
  });

  it("shows the project label", () => {
    render(
      <SessionList
        sessions={[mk({ projectLabel: "api-service ▸ courier" })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline={null}
      />,
    );
    expect(screen.getByText("api-service ▸ courier")).toBeDefined();
  });

  it("falls back to a placeholder when a session has no title", () => {
    render(
      <SessionList
        sessions={[mk({ aiTitle: null })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline={null}
      />,
    );
    expect(screen.getByText("Untitled session")).toBeDefined();
  });

  it("shows an empty state when there are no sessions", () => {
    render(
      <SessionList sessions={[]} selectedId={null} onSelect={vi.fn()} versionBaseline={null} />,
    );
    expect(screen.getByText(/no sessions/i)).toBeDefined();
  });

  it("shows the project label once, in the group header only", () => {
    render(
      <SessionList
        sessions={[
          mk({ sessionId: "a", projectLabel: "api-service ▸ courier" }),
          mk({ sessionId: "b", projectLabel: "api-service ▸ courier" }),
        ]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline={null}
      />,
    );
    // Two sessions in one project must still yield exactly one label element.
    expect(screen.getAllByText("api-service ▸ courier")).toHaveLength(1);
  });

  it("shows the git branch on the row", () => {
    render(
      <SessionList
        sessions={[mk({ gitBranch: "feat/courier" })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline={null}
      />,
    );
    expect(screen.getByText("feat/courier")).toBeDefined();
  });

  it("shows the relative age of the session's last activity", () => {
    const twoHoursAgo = Math.floor(Date.now() / 1000) - 7200;
    render(
      <SessionList
        sessions={[mk({ lastActivity: twoHoursAgo })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline={null}
      />,
    );
    expect(screen.getByText("2h")).toBeDefined();
  });

  it("shows a humanized status badge rather than the raw enum value", () => {
    render(
      <SessionList
        sessions={[
          mk({ annotation: { notes: "", status: "waitingOnMe", displayName: null } }),
        ]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline={null}
      />,
    );
    expect(screen.getByText("Waiting on me")).toBeDefined();
    expect(screen.queryByText("waitingOnMe")).toBeNull();
  });

  it("shows the session's Claude version", () => {
    render(
      <SessionList
        sessions={[mk({ version: "2.1.205" })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline="2.1.220"
      />,
    );
    expect(screen.getByText("v2.1.205")).toBeDefined();
  });

  it("renders no version badge when the session has none", () => {
    render(
      <SessionList
        sessions={[mk({ version: null })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline="2.1.220"
      />,
    );
    // Anchored to the exact badge format. A loose /^v/ would also match the
    // branch name or title and pass for the wrong reason.
    expect(screen.queryByText(/^v\d+\.\d+/)).toBeNull();
  });

  it("styles an outdated version differently from a current one", () => {
    const { rerender } = render(
      <SessionList
        sessions={[mk({ version: "2.1.205", liveness: "legacy" })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline="2.1.220"
      />,
    );
    const stale = screen.getByText("v2.1.205").className;

    rerender(
      <SessionList
        sessions={[mk({ version: "2.1.220", liveness: "legacy" })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline="2.1.220"
      />,
    );
    const current = screen.getByText("v2.1.220").className;

    expect(stale).not.toBe(current);
  });

  it("does not highlight an idle session that is behind", () => {
    // Amber means "something running right now is stale". An idle session is
    // old by definition and resuming it picks up the current binary.
    const { rerender } = render(
      <SessionList
        sessions={[mk({ version: "2.1.205", liveness: "idle" })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline="2.1.220"
      />,
    );
    const idleStale = screen.getByText("v2.1.205").className;

    rerender(
      <SessionList
        sessions={[mk({ version: "2.1.220", liveness: "idle" })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline="2.1.220"
      />,
    );
    expect(screen.getByText("v2.1.220").className).toBe(idleStale);
  });

  it("does not mark anything stale when there is no baseline", () => {
    const { rerender } = render(
      <SessionList
        sessions={[mk({ version: "2.1.205" })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline={null}
      />,
    );
    const noBaseline = screen.getByText("v2.1.205").className;

    rerender(
      <SessionList
        sessions={[mk({ version: "2.1.220" })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline={null}
      />,
    );
    expect(screen.getByText("v2.1.220").className).toBe(noBaseline);
  });

  it("lifts live sessions into a pinned Active group above the project groups", () => {
    // The regression this guards: grouping by project put a group's position at
    // the mercy of its first-seen member, so a live session in a mostly-idle
    // project rendered below entire idle groups.
    render(
      <SessionList
        sessions={[
          mk({ sessionId: "idle-1", projectLabel: "api-service", liveness: "idle", aiTitle: "Idle one" }),
          mk({ sessionId: "idle-2", projectLabel: "api-service", liveness: "idle", aiTitle: "Idle two" }),
          mk({ sessionId: "live-1", projectLabel: "claudron", liveness: "legacy", aiTitle: "Live one" }),
        ]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline={null}
      />,
    );
    const headings = screen.getAllByRole("heading").map((h) => h.textContent);
    expect(headings[0]).toBe("Active");
    expect(screen.getByText("Live one")).toBeDefined();
  });

  it("renders no Active group when nothing is live", () => {
    render(
      <SessionList
        sessions={[mk({ liveness: "idle", projectLabel: "api-service" })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline={null}
      />,
    );
    expect(screen.queryByText("Active")).toBeNull();
  });

  it("keeps a live session out of its own project group", () => {
    render(
      <SessionList
        sessions={[
          mk({ sessionId: "live-1", projectLabel: "api-service", liveness: "legacy", aiTitle: "Live one" }),
          mk({ sessionId: "idle-1", projectLabel: "api-service", liveness: "idle", aiTitle: "Idle one" }),
        ]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline={null}
      />,
    );
    const headings = screen.getAllByRole("heading").map((h) => h.textContent);
    expect(headings).toEqual(["Active", "api-service"]);
  });
});
