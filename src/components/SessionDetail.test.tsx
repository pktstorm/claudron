import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import type { Session } from "../types";

const focusSession = vi.fn();
const resumeSession = vi.fn();
vi.mock("../api/tauri", () => ({
  focusSession: (...a: unknown[]) => focusSession(...a),
  resumeSession: (...a: unknown[]) => resumeSession(...a),
}));

import { SessionDetail } from "./SessionDetail";

const mk = (over: Partial<Session> = {}): Session => ({
  sessionId: "s1",
  aiTitle: "Fix the parser",
  lastPrompt: "make it work",
  gitBranch: "main",
  cwd: "/code/repo",
  projectLabel: "repo",
  version: "2.1.220",
  lastActivity: 100,
  liveness: "idle",
  annotation: { notes: "", status: null, displayName: null },
  ...over,
});

describe("SessionDetail", () => {
  beforeEach(() => {
    focusSession.mockReset();
    resumeSession.mockReset();
    focusSession.mockResolvedValue(undefined);
    resumeSession.mockResolvedValue(undefined);
  });

  it("shows an empty state when nothing is selected", () => {
    render(<SessionDetail session={null} onAnnotationChange={vi.fn()} />);
    expect(screen.getByText(/select a session/i)).toBeDefined();
  });

  it("shows the last prompt and branch", () => {
    render(<SessionDetail session={mk()} onAnnotationChange={vi.fn()} />);
    expect(screen.getByText("make it work")).toBeDefined();
    expect(screen.getByText("main")).toBeDefined();
  });

  it("emits annotation changes when notes are edited", () => {
    const onChange = vi.fn();
    render(<SessionDetail session={mk()} onAnnotationChange={onChange} />);
    fireEvent.change(screen.getByPlaceholderText(/notes/i), { target: { value: "new note" } });
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ notes: "new note" }),
    );
  });

  it("offers Resume for an idle session", () => {
    render(<SessionDetail session={mk({ liveness: "idle" })} onAnnotationChange={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /resume/i }));
    expect(resumeSession).toHaveBeenCalledWith("s1", "/code/repo");
  });

  it("offers Jump to terminal for a running session", () => {
    render(<SessionDetail session={mk({ liveness: "legacy" })} onAnnotationChange={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /jump/i }));
    expect(focusSession).toHaveBeenCalledWith("/code/repo");
  });

  it("does not offer Jump for a session with no live process", () => {
    render(<SessionDetail session={mk({ liveness: "idle" })} onAnnotationChange={vi.fn()} />);
    expect(screen.queryByRole("button", { name: /jump/i })).toBeNull();
  });

  it("surfaces an error when jumping to the terminal fails", async () => {
    focusSession.mockRejectedValue(new Error("iTerm2 is not running"));
    render(<SessionDetail session={mk({ liveness: "legacy" })} onAnnotationChange={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /jump/i }));
    expect(await screen.findByRole("alert")).toBeDefined();
  });

  it("surfaces an error when resume fails", async () => {
    resumeSession.mockRejectedValue(new Error("osascript failed"));
    render(<SessionDetail session={mk({ liveness: "idle" })} onAnnotationChange={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /resume/i }));
    expect(await screen.findByRole("alert")).toBeDefined();
  });

  it("says the status blocks are user-set, not detected", () => {
    render(<SessionDetail session={mk()} onAnnotationChange={vi.fn()} />);
    expect(screen.getByText("Your status")).toBeDefined();
    expect(screen.getByText(/not detected/i)).toBeDefined();
  });
});
