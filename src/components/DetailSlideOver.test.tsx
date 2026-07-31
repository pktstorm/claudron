import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent, within } from "@testing-library/react";
import type { Session } from "../types";

const gitLocal = vi.fn();
const gitRemote = vi.fn();
vi.mock("../api/git", () => ({
  gitLocal: (...a: unknown[]) => gitLocal(...a),
  gitRemote: (...a: unknown[]) => gitRemote(...a),
  removeWorktree: vi.fn(),
}));

const focusSession = vi.fn();
const resumeSession = vi.fn();
vi.mock("../api/tauri", () => ({
  focusSession: (...a: unknown[]) => focusSession(...a),
  resumeSession: (...a: unknown[]) => resumeSession(...a),
}));

import { DetailSlideOver } from "./DetailSlideOver";

const mk = (over: Partial<Session> = {}): Session => ({
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

describe("DetailSlideOver", () => {
  beforeEach(() => {
    gitLocal.mockReset();
    gitRemote.mockReset();
    gitRemote.mockResolvedValue({ pullRequest: null });
    gitLocal.mockResolvedValue({
      branch: "feature-x",
      dirtyCount: 0,
      ahead: 0,
      behind: 0,
      worktree: { isWorktree: true, path: "/code/repo-wt", repoRoot: "/code/repo" },
      defaultBranch: "main",
      mergedIntoDefault: null,
    });
  });

  it("resets to the Overview tab when a different session is selected", async () => {
    const session1 = mk({ sessionId: "s1" });
    const session2 = mk({ sessionId: "s2" });
    const { rerender } = render(
      <DetailSlideOver session={session1} onAnnotationChange={vi.fn()} onClose={vi.fn()} />,
    );

    fireEvent.click(screen.getByRole("button", { name: /^git$/i }));
    await waitFor(() =>
      expect(within(screen.getByTestId("detail-slideover")).getByText("feature-x")).toBeDefined(),
    );

    // Selecting a different session (same component instance, as App keeps
    // DetailSlideOver mounted across a selection change) must snap the tab
    // back to Overview rather than keep showing Git content computed for
    // the previous session's cwd.
    rerender(<DetailSlideOver session={session2} onAnnotationChange={vi.fn()} onClose={vi.fn()} />);
    expect(screen.getByPlaceholderText("Notes for this session…")).toBeDefined();
    expect(screen.queryByText("feature-x")).toBeNull();
  });

  it("wires isLive from a managed session into the Git tab's Remove button", async () => {
    const session = mk({ liveness: "managed" });
    render(<DetailSlideOver session={session} onAnnotationChange={vi.fn()} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /^git$/i }));
    const btn = await screen.findByRole("button", { name: /remove worktree/i });
    expect(btn.hasAttribute("disabled")).toBe(true);
  });

  it("wires isLive from an idle session as false into the Git tab's Remove button", async () => {
    const session = mk({ liveness: "idle" });
    render(<DetailSlideOver session={session} onAnnotationChange={vi.fn()} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /^git$/i }));
    const btn = await screen.findByRole("button", { name: /remove worktree/i });
    expect(btn.hasAttribute("disabled")).toBe(false);
  });
});
