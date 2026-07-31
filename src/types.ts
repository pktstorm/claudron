export type Liveness = "managed" | "legacy" | "interrupted" | "idle";

export type ManualStatus = "blocked" | "needsReview" | "waitingOnMe" | "background";

export interface Annotation {
  notes: string;
  status: ManualStatus | null;
  displayName: string | null;
}

export interface Session {
  sessionId: string;
  aiTitle: string | null;
  lastPrompt: string | null;
  gitBranch: string | null;
  cwd: string;
  projectLabel: string;
  version: string | null;
  lastActivity: number;
  liveness: Liveness;
  annotation: Annotation;
}

export interface SessionList {
  sessions: Session[];
  versionBaseline: string | null;
}
