import { invoke } from "@tauri-apps/api/core";
import type { Annotation, SessionList } from "../types";

export function listSessions(): Promise<SessionList> {
  return invoke("list_sessions");
}

export function setAnnotation(sessionId: string, annotation: Annotation): Promise<void> {
  return invoke("set_annotation", { sessionId, annotation });
}

export function focusSession(cwd: string): Promise<void> {
  return invoke("focus_session", { cwd });
}

export function resumeSession(sessionId: string, cwd: string): Promise<void> {
  return invoke("resume_session", { sessionId, cwd });
}
