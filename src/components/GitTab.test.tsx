import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import type { GitLocal } from "../types/git";

const gitLocal = vi.fn();
const gitRemote = vi.fn();
const removeWorktree = vi.fn();
vi.mock("../api/git", () => ({
  gitLocal: (...a: unknown[]) => gitLocal(...a),
  gitRemote: (...a: unknown[]) => gitRemote(...a),
  removeWorktree: (...a: unknown[]) => removeWorktree(...a),
}));

import { GitTab } from "./GitTab";

const local = (over: Partial<GitLocal> = {}): GitLocal => ({
  branch: "feature",
  dirtyCount: 0,
  ahead: 0,
  behind: 0,
  worktree: { isWorktree: true, path: "/repo/wt", repoRoot: "/repo" },
  defaultBranch: "main",
  mergedIntoDefault: false,
  ...over,
});

describe("GitTab", () => {
  beforeEach(() => {
    gitLocal.mockReset();
    gitRemote.mockReset();
    removeWorktree.mockReset();
    gitRemote.mockResolvedValue({ pullRequest: null });
  });

  it("shows the branch and a clean state", async () => {
    gitLocal.mockResolvedValue(local());
    render(<GitTab cwd="/repo/wt" isLive={false} />);
    await waitFor(() => expect(screen.getByText("feature")).toBeDefined());
    expect(screen.getByText(/clean/i)).toBeDefined();
  });

  it("shows the changed-file count when dirty", async () => {
    gitLocal.mockResolvedValue(local({ dirtyCount: 3 }));
    render(<GitTab cwd="/repo/wt" isLive={false} />);
    await waitFor(() => expect(screen.getByText(/3 changed/i)).toBeDefined());
  });

  it("says no upstream rather than showing zeroes", async () => {
    // null ahead/behind means nothing is tracked -- showing 0/0 would read as
    // "in sync", which is the opposite of the truth.
    gitLocal.mockResolvedValue(local({ ahead: null, behind: null }));
    render(<GitTab cwd="/repo/wt" isLive={false} />);
    await waitFor(() => expect(screen.getByText(/no upstream/i)).toBeDefined());
  });

  it("labels ahead/behind against upstream, not the default branch", async () => {
    // ahead/behind is measured against @{upstream} (origin/<branch> for a
    // feature branch), never against defaultBranch. branch: "feature" and
    // defaultBranch: "main" differ here, so a label naming "main" would be
    // describing a different comparison than the one actually computed.
    gitLocal.mockResolvedValue(local({ ahead: 1, behind: 0 }));
    render(<GitTab cwd="/repo/wt" isLive={false} />);
    await waitFor(() => expect(screen.getByText(/1 ahead/i)).toBeDefined());
    expect(screen.queryByText(/main/)).toBeNull();
  });

  it("renders local blocks before the pull request resolves", async () => {
    gitLocal.mockResolvedValue(local());
    // gh never settles during this test.
    gitRemote.mockReturnValue(new Promise(() => {}));
    render(<GitTab cwd="/repo/wt" isLive={false} />);
    // Local content is present while the PR block is still loading.
    await waitFor(() => expect(screen.getByText("feature")).toBeDefined());
    expect(screen.getByText(/loading pull request/i)).toBeDefined();
  });

  it("explains when gh is unavailable without hiding local data", async () => {
    gitLocal.mockResolvedValue(local());
    gitRemote.mockRejectedValue(new Error("gh is not installed or not on PATH"));
    render(<GitTab cwd="/repo/wt" isLive={false} />);
    await waitFor(() => expect(screen.getByText(/gh is not installed/i)).toBeDefined());
    expect(screen.getByText("feature")).toBeDefined();
  });

  it("shows a pull request with its check rollup", async () => {
    gitLocal.mockResolvedValue(local());
    gitRemote.mockResolvedValue({
      pullRequest: {
        number: 334,
        title: "fix the thing",
        state: "OPEN",
        checks: { state: "failing", passing: 2, failing: 1, pending: 0 },
      },
    });
    render(<GitTab cwd="/repo/wt" isLive={false} />);
    await waitFor(() => expect(screen.getByText(/#334/)).toBeDefined());
    expect(screen.getByText("fix the thing")).toBeDefined();
  });

  it("disables removal while a live process is in the directory", async () => {
    gitLocal.mockResolvedValue(local());
    render(<GitTab cwd="/repo/wt" isLive={true} />);
    const btn = await screen.findByRole("button", { name: /remove worktree/i });
    expect(btn.hasAttribute("disabled")).toBe(true);
    expect(screen.getByText(/session is running/i)).toBeDefined();
  });

  it("disables removal when the worktree is dirty", async () => {
    gitLocal.mockResolvedValue(local({ dirtyCount: 2 }));
    render(<GitTab cwd="/repo/wt" isLive={false} />);
    const btn = await screen.findByRole("button", { name: /remove worktree/i });
    expect(btn.hasAttribute("disabled")).toBe(true);
  });

  it("offers no removal control for a main checkout", async () => {
    gitLocal.mockResolvedValue(
      local({ worktree: { isWorktree: false, path: "/repo", repoRoot: "/repo" } }),
    );
    render(<GitTab cwd="/repo" isLive={false} />);
    await waitFor(() => expect(screen.getByText("feature")).toBeDefined());
    expect(screen.queryByRole("button", { name: /remove worktree/i })).toBeNull();
  });

  it("removes the worktree's own path, not merely the session cwd", async () => {
    // cwd and worktree.path coincide in every other fixture, which would let
    // a bug that passes `cwd` to removeWorktree slip through undetected. Here
    // they diverge (a symlinked/resolved path vs. the raw cwd) so the call
    // must specifically use worktree.path.
    gitLocal.mockResolvedValue(
      local({ worktree: { isWorktree: true, path: "/real/wt", repoRoot: "/repo" } }),
    );
    removeWorktree.mockResolvedValue(undefined);
    render(<GitTab cwd="/repo/wt" isLive={false} />);

    fireEvent.click(await screen.findByRole("button", { name: /remove worktree/i }));
    fireEvent.click(screen.getByRole("button", { name: /^confirm/i }));
    await waitFor(() => expect(removeWorktree).toHaveBeenCalledWith("/real/wt"));
    expect(removeWorktree).not.toHaveBeenCalledWith("/repo/wt");
  });

  it("requires confirmation naming the path before removing", async () => {
    gitLocal.mockResolvedValue(local());
    removeWorktree.mockResolvedValue(undefined);
    render(<GitTab cwd="/repo/wt" isLive={false} />);

    fireEvent.click(await screen.findByRole("button", { name: /remove worktree/i }));
    // Nothing is removed until the confirmation is accepted.
    expect(removeWorktree).not.toHaveBeenCalled();
    expect(screen.getByText("/repo/wt")).toBeDefined();

    fireEvent.click(screen.getByRole("button", { name: /^confirm/i }));
    await waitFor(() => expect(removeWorktree).toHaveBeenCalledWith("/repo/wt"));
  });

  it("shows merge status as unknown rather than guessing not-merged", async () => {
    // mergedIntoDefault: null means undeterminable. Showing "not merged" would
    // be a confident-looking lie; the confirmation must say unknown instead.
    gitLocal.mockResolvedValue(local({ mergedIntoDefault: null }));
    render(<GitTab cwd="/repo/wt" isLive={false} />);
    fireEvent.click(await screen.findByRole("button", { name: /remove worktree/i }));
    expect(screen.getByText(/unknown/i)).toBeDefined();
    expect(screen.queryByText(/not merged/i)).toBeNull();
  });

  it("shows merge status as merged when true", async () => {
    gitLocal.mockResolvedValue(local({ mergedIntoDefault: true }));
    render(<GitTab cwd="/repo/wt" isLive={false} />);
    fireEvent.click(await screen.findByRole("button", { name: /remove worktree/i }));
    expect(screen.getByText(/— merged/)).toBeDefined();
  });

  it("shows merge status as not merged when false", async () => {
    gitLocal.mockResolvedValue(local({ mergedIntoDefault: false }));
    render(<GitTab cwd="/repo/wt" isLive={false} />);
    fireEvent.click(await screen.findByRole("button", { name: /remove worktree/i }));
    expect(screen.getByText(/— not merged/)).toBeDefined();
  });

  it("shows a success message and hides the worktree section after removal", async () => {
    gitLocal.mockResolvedValue(local());
    removeWorktree.mockResolvedValue(undefined);
    render(<GitTab cwd="/repo/wt" isLive={false} />);

    fireEvent.click(await screen.findByRole("button", { name: /remove worktree/i }));
    fireEvent.click(screen.getByRole("button", { name: /^confirm/i }));

    await waitFor(() => expect(screen.getByText(/worktree removed/i)).toBeDefined());
    expect(screen.queryByRole("button", { name: /remove worktree/i })).toBeNull();
    expect(screen.queryByText(/^\/repo\/wt$/)).toBeNull();
  });

  it("closes the confirmation when a refresh loads fresh data", async () => {
    // Fresh data may no longer match the premises the confirmation named
    // (e.g. dirtyCount going from 0 to 5) -- a stale confirmation that is
    // still clickable would let the user confirm against a state they never
    // actually saw described.
    gitLocal.mockResolvedValue(local());
    render(<GitTab cwd="/repo/wt" isLive={false} />);

    fireEvent.click(await screen.findByRole("button", { name: /remove worktree/i }));
    expect(screen.getByRole("button", { name: /^confirm/i })).toBeDefined();

    gitLocal.mockResolvedValue(local({ dirtyCount: 5 }));
    fireEvent.click(screen.getByRole("button", { name: /^refresh$/i }));

    await waitFor(() => expect(screen.getByText(/5 changed/i)).toBeDefined());
    expect(screen.queryByRole("button", { name: /^confirm/i })).toBeNull();
  });

  it("closes the confirmation when the session becomes live", async () => {
    gitLocal.mockResolvedValue(local());
    const { rerender } = render(<GitTab cwd="/repo/wt" isLive={false} />);

    fireEvent.click(await screen.findByRole("button", { name: /remove worktree/i }));
    expect(screen.getByRole("button", { name: /^confirm/i })).toBeDefined();

    rerender(<GitTab cwd="/repo/wt" isLive={true} />);

    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /^confirm/i })).toBeNull(),
    );
  });

  it("surfaces git's own message when removal fails", async () => {
    gitLocal.mockResolvedValue(local());
    removeWorktree.mockRejectedValue(new Error("fatal: validation failed"));
    render(<GitTab cwd="/repo/wt" isLive={false} />);
    fireEvent.click(await screen.findByRole("button", { name: /remove worktree/i }));
    fireEvent.click(screen.getByRole("button", { name: /^confirm/i }));
    await waitFor(() => expect(screen.getByText(/validation failed/i)).toBeDefined());
  });

  it("reports when the directory is not a git repository", async () => {
    gitLocal.mockRejectedValue(new Error("not a git working tree"));
    render(<GitTab cwd="/tmp/nope" isLive={false} />);
    await waitFor(() => expect(screen.getByText(/not a git working tree/i)).toBeDefined());
  });

  it("shows a terminal message instead of an endless spinner for detached HEAD", async () => {
    // branch: null means detached HEAD. The remote effect early-returns on a
    // null branch, so gitRemote is never called and `remote` stays null
    // forever -- the render must not mistake that for "still loading".
    gitLocal.mockResolvedValue(local({ branch: null }));
    render(<GitTab cwd="/repo/wt" isLive={false} />);
    await waitFor(() => expect(screen.getByText(/detached head/i)).toBeDefined());
    expect(screen.queryByText(/loading pull request/i)).toBeNull();
    expect(screen.getByText(/no branch.*cannot look up a pull request/i)).toBeDefined();
    expect(gitRemote).not.toHaveBeenCalled();
  });
});
