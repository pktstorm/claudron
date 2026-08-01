import { invoke } from "@tauri-apps/api/core";
import type { HookPlan } from "../types/hooks";

/** Describe the current install without changing anything. */
export function hookStatus(): Promise<HookPlan> {
  return invoke<HookPlan>("hook_status");
}

/** Install the hook. Only ever called after explicit consent. */
export function installHooks(): Promise<HookPlan> {
  return invoke<HookPlan>("install_hooks");
}

export function uninstallHooks(): Promise<HookPlan> {
  return invoke<HookPlan>("uninstall_hooks");
}
