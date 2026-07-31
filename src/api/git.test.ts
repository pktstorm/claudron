import { describe, it, expect, vi, beforeEach } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import { gitLocal, gitRemote, removeWorktree } from "./git";

describe("git api", () => {
  beforeEach(() => invoke.mockReset());

  it("gitLocal passes the cwd", async () => {
    invoke.mockResolvedValue({});
    await gitLocal("/repo");
    expect(invoke).toHaveBeenCalledWith("git_local", { cwd: "/repo" });
  });

  it("gitRemote passes the cwd and branch", async () => {
    invoke.mockResolvedValue({ pullRequest: null });
    await gitRemote("/repo", "feature");
    expect(invoke).toHaveBeenCalledWith("git_remote", { cwd: "/repo", branch: "feature" });
  });

  it("removeWorktree passes the path", async () => {
    invoke.mockResolvedValue(undefined);
    await removeWorktree("/repo/wt");
    expect(invoke).toHaveBeenCalledWith("remove_worktree", { path: "/repo/wt" });
  });
});
