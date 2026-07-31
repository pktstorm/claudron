import { describe, it, expect, vi, beforeEach } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import { listSessions, setAnnotation, focusSession, resumeSession } from "./tauri";

describe("tauri api", () => {
  beforeEach(() => invoke.mockReset());

  it("listSessions calls the list_sessions command", async () => {
    invoke.mockResolvedValue([]);
    await listSessions();
    expect(invoke).toHaveBeenCalledWith("list_sessions");
  });

  it("listSessions returns the session list wrapper", async () => {
    invoke.mockResolvedValue({ sessions: [], versionBaseline: "2.1.220" });
    const out = await listSessions();
    expect(invoke).toHaveBeenCalledWith("list_sessions");
    expect(out.versionBaseline).toBe("2.1.220");
    expect(out.sessions).toHaveLength(0);
  });

  it("setAnnotation passes sessionId and annotation", async () => {
    invoke.mockResolvedValue(undefined);
    const annotation = { notes: "n", status: null, displayName: null };
    await setAnnotation("s1", annotation);
    expect(invoke).toHaveBeenCalledWith("set_annotation", { sessionId: "s1", annotation });
  });

  it("focusSession passes the cwd", async () => {
    invoke.mockResolvedValue(undefined);
    await focusSession("/repo");
    expect(invoke).toHaveBeenCalledWith("focus_session", { cwd: "/repo" });
  });

  it("resumeSession passes sessionId and cwd", async () => {
    invoke.mockResolvedValue(undefined);
    await resumeSession("s1", "/repo");
    expect(invoke).toHaveBeenCalledWith("resume_session", { sessionId: "s1", cwd: "/repo" });
  });
});
