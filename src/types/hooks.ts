/** Whether Claudron's hook is registered in the user's Claude Code settings. */
export type InstallState = "installed" | "partial" | "notInstalled";

/** What the UI needs to describe an install before the user consents to it. */
export interface HookPlan {
  state: InstallState;
  /** The user's settings.json — a file Claudron does not own. */
  settingsPath: string;
  /** Where Claudron's own hook script lives. */
  scriptPath: string;
  /** The lifecycle events that will be registered. */
  events: string[];
  /** Exactly what will be added to settings.json, for display before consent. */
  settingsSnippet: string;
}
