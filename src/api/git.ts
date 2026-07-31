import { invoke } from "@tauri-apps/api/core";
import type { GitLocal, GitRemote } from "../types/git";

export function gitLocal(cwd: string): Promise<GitLocal> {
  return invoke("git_local", { cwd });
}

export function gitRemote(cwd: string, branch: string): Promise<GitRemote> {
  return invoke("git_remote", { cwd, branch });
}

export function removeWorktree(path: string): Promise<void> {
  return invoke("remove_worktree", { path });
}
