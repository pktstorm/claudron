export interface WorktreeInfo {
  isWorktree: boolean;
  path: string;
  repoRoot: string;
}

export interface GitLocal {
  branch: string | null;
  dirtyCount: number;
  /** null means no upstream at all; 0 means in sync. They are not the same. */
  ahead: number | null;
  behind: number | null;
  worktree: WorktreeInfo;
  defaultBranch: string | null;
  /** null means undeterminable — render as unknown, never as "not merged". */
  mergedIntoDefault: boolean | null;
}

export type CheckState = "passing" | "failing" | "pending" | "none";

export interface CheckRollup {
  state: CheckState;
  passing: number;
  failing: number;
  pending: number;
}

export interface PullRequest {
  number: number;
  title: string;
  state: string;
  checks: CheckRollup;
}

export interface GitRemote {
  pullRequest: PullRequest | null;
}
