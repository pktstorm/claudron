import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import type { Session } from "./types";

const listSessions = vi.fn();
const setAnnotation = vi.fn();
vi.mock("./api/tauri", () => ({
  listSessions: () => listSessions(),
  setAnnotation: (...a: unknown[]) => setAnnotation(...a),
  focusSession: vi.fn(),
  resumeSession: vi.fn(),
}));

vi.mock("./api/conversation", () => ({
  loadConversation: vi.fn().mockResolvedValue({ sessionId: "s1", turns: [], offset: 0 }),
  pollConversation: vi.fn().mockResolvedValue({ turns: [], updates: [], offset: 0, reset: false }),
  loadSubagent: vi.fn().mockResolvedValue({ sessionId: "s1", turns: [], offset: 0 }),
}));

const gitLocal = vi.fn();
vi.mock("./api/scan", () => ({
  // Subscribing fails outside Tauri. The app must survive that: a progress bar
  // is decoration, and an unrejected promise here surfaced as an unhandled
  // rejection that failed CI.
  onScanProgress: vi.fn(() => Promise.reject(new Error("no tauri event bridge"))),
}));

vi.mock("./api/git", () => ({
  gitLocal: (...a: unknown[]) => gitLocal(...a),
  gitRemote: vi.fn().mockResolvedValue({ pullRequest: null }),
  removeWorktree: vi.fn(),
}));

import App from "./App";

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

describe("App", () => {
  beforeEach(() => {
    listSessions.mockReset();
    setAnnotation.mockReset();
    gitLocal.mockReset();
    gitLocal.mockResolvedValue({
      branch: "main",
      dirtyCount: 0,
      ahead: 0,
      behind: 0,
      worktree: { isWorktree: false, path: "/code/repo", repoRoot: "/code/repo" },
      defaultBranch: "main",
      mergedIntoDefault: null,
    });
  });

  it("renders sessions returned by the backend", async () => {
    listSessions.mockResolvedValue({ sessions: [mk()], versionBaseline: "2.1.220" });
    render(<App />);
    await waitFor(() => expect(screen.getByText("Fix the parser")).toBeDefined());
  });

  it("still renders when scan progress cannot be subscribed to", async () => {
    // ./api/scan is mocked to reject: outside Tauri there is no event bridge.
    // A progress bar is decoration -- failing to subscribe must not take the
    // app down.
    //
    // HONEST NOTE: this assertion alone does not catch a missing `.catch()`.
    // Without it the render still succeeds and this test still passes; what
    // changes is that Vitest reports "Unhandled Errors", which fails CI. The
    // load-bearing part is the rejecting mock above, which makes the rejection
    // happen at all -- the assertion just pins that the app survives it.
    listSessions.mockResolvedValue({ sessions: [mk()], versionBaseline: null });
    render(<App />);
    await waitFor(() => expect(screen.getByText("Fix the parser")).toBeDefined());
  });

  it("shows a count of loaded sessions", async () => {
    listSessions.mockResolvedValue({
      sessions: [mk({ sessionId: "a" }), mk({ sessionId: "b" })],
      versionBaseline: "2.1.220",
    });
    render(<App />);
    await waitFor(() => expect(screen.getByText(/2 sessions/i)).toBeDefined());
  });

  it("surfaces an error state when the backend fails", async () => {
    listSessions.mockRejectedValue(new Error("boom"));
    render(<App />);
    await waitFor(() => expect(screen.getByText(/could not load sessions/i)).toBeDefined());
  });

  it("tells the user when a note fails to save", async () => {
    listSessions.mockResolvedValue({ sessions: [mk()], versionBaseline: "2.1.220" });
    setAnnotation.mockRejectedValue(new Error("annotation store is unreadable"));
    render(<App />);
    await waitFor(() => expect(screen.getByText("Fix the parser")).toBeDefined());
    fireEvent.click(screen.getByText("Fix the parser"));
    fireEvent.click(screen.getByRole("button", { name: /details/i }));
    fireEvent.change(screen.getByPlaceholderText("Notes for this session…"), {
      target: { value: "x" },
    });
    expect(await screen.findByRole("alert", {}, { timeout: 3000 })).toBeDefined();
  });

  it("shows the conversation pane once a session is selected", async () => {
    listSessions.mockResolvedValue({ sessions: [mk()], versionBaseline: "2.1.220" });
    render(<App />);
    await waitFor(() => expect(screen.getByText("Fix the parser")).toBeDefined());
    fireEvent.click(screen.getByText("Fix the parser"));
    // The conversation pane owns the right side now.
    await waitFor(() => expect(screen.getByTestId("conversation-pane")).toBeDefined());
  });

  it("opens the details slide-over on demand", async () => {
    listSessions.mockResolvedValue({ sessions: [mk()], versionBaseline: "2.1.220" });
    render(<App />);
    await waitFor(() => expect(screen.getByText("Fix the parser")).toBeDefined());
    fireEvent.click(screen.getByText("Fix the parser"));
    expect(screen.queryByTestId("detail-slideover")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /details/i }));
    expect(screen.getByTestId("detail-slideover")).toBeDefined();
  });

  it("opens the slide-over on Overview, not Git", async () => {
    listSessions.mockResolvedValue({ sessions: [mk()], versionBaseline: "2.1.220" });
    render(<App />);
    await waitFor(() => expect(screen.getByText("Fix the parser")).toBeDefined());
    fireEvent.click(screen.getByText("Fix the parser"));
    fireEvent.click(screen.getByRole("button", { name: /details/i }));
    // Overview is instant; Git would show a loading state first.
    expect(screen.getByPlaceholderText("Notes for this session…")).toBeDefined();
  });

  it("switches to the Git tab on demand", async () => {
    listSessions.mockResolvedValue({ sessions: [mk()], versionBaseline: "2.1.220" });
    render(<App />);
    await waitFor(() => expect(screen.getByText("Fix the parser")).toBeDefined());
    fireEvent.click(screen.getByText("Fix the parser"));
    fireEvent.click(screen.getByRole("button", { name: /details/i }));
    fireEvent.click(screen.getByRole("button", { name: /^git$/i }));
    // "main" alone is not proof: SessionRow renders session.gitBranch in the
    // always-mounted list, so that text matches regardless of whether GitTab
    // mounted. The Refresh button only exists inside GitTab.
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /^refresh$/i })).toBeDefined(),
    );
  });

  it("disables worktree removal in the Git tab for a live session", async () => {
    listSessions.mockResolvedValue({
      sessions: [mk({ liveness: "managed" })],
      versionBaseline: "2.1.220",
    });
    gitLocal.mockResolvedValue({
      branch: "main",
      dirtyCount: 0,
      ahead: 0,
      behind: 0,
      worktree: { isWorktree: true, path: "/code/repo-wt", repoRoot: "/code/repo" },
      defaultBranch: "main",
      mergedIntoDefault: null,
    });
    render(<App />);
    // Wait for this test's own fixture (the "Managed" badge), not just the
    // title text, since a prior test's cached query result can otherwise
    // still be on screen when this render first paints.
    await waitFor(() => expect(screen.getByText("Managed")).toBeDefined());
    fireEvent.click(screen.getByText("Fix the parser"));
    fireEvent.click(screen.getByRole("button", { name: /details/i }));
    fireEvent.click(screen.getByRole("button", { name: /^git$/i }));
    const btn = await screen.findByRole("button", { name: /remove worktree/i });
    expect(btn.hasAttribute("disabled")).toBe(true);
  });

  it("leaves worktree removal enabled in the Git tab for a non-live session", async () => {
    listSessions.mockResolvedValue({
      sessions: [mk({ liveness: "idle", aiTitle: "Idle session for removal test" })],
      versionBaseline: "2.1.220",
    });
    gitLocal.mockResolvedValue({
      branch: "main",
      dirtyCount: 0,
      ahead: 0,
      behind: 0,
      worktree: { isWorktree: true, path: "/code/repo-wt", repoRoot: "/code/repo" },
      defaultBranch: "main",
      mergedIntoDefault: null,
    });
    render(<App />);
    // Wait for this test's own fixture title, not just any stale cached
    // session, since a prior test's cached query result can otherwise still
    // be on screen when this render first paints.
    await waitFor(() => expect(screen.getByText("Idle session for removal test")).toBeDefined());
    fireEvent.click(screen.getByText("Idle session for removal test"));
    fireEvent.click(screen.getByRole("button", { name: /details/i }));
    fireEvent.click(screen.getByRole("button", { name: /^git$/i }));
    const btn = await screen.findByRole("button", { name: /remove worktree/i });
    expect(btn.hasAttribute("disabled")).toBe(false);
  });
});
