import type { Liveness, ManualStatus } from "./types";

export const LIVENESS_LABEL: Record<Liveness, string> = {
  managed: "Managed",
  legacy: "Active here",
  interrupted: "Interrupted",
  idle: "Idle",
};

export const STATUS_LABEL: Record<ManualStatus, string> = {
  blocked: "Blocked",
  needsReview: "Needs review",
  waitingOnMe: "Waiting on me",
  background: "Background",
};
