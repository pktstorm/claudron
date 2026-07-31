import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { Turn } from "../types/conversation";

const loadConversation = vi.fn();
const pollConversation = vi.fn();
const loadSubagent = vi.fn();
vi.mock("../api/conversation", () => ({
  loadConversation: (...a: unknown[]) => loadConversation(...a),
  pollConversation: (...a: unknown[]) => pollConversation(...a),
  loadSubagent: (...a: unknown[]) => loadSubagent(...a),
}));

import { ConversationPane, applyDelta } from "./ConversationPane";

const turn = (uuid: string, text: string): Turn => ({
  uuid,
  role: "assistant",
  timestamp: "2026-07-31T00:00:00Z",
  blocks: [{ kind: "text", text }],
  model: null,
  usage: null,
});

function wrap(ui: React.ReactElement) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={qc}>{ui}</QueryClientProvider>;
}

describe("ConversationPane", () => {
  beforeEach(() => {
    loadConversation.mockReset();
    pollConversation.mockReset();
    loadSubagent.mockReset();
    pollConversation.mockResolvedValue({ turns: [], updates: [], offset: 0, reset: false });
  });

  it("renders the loaded turns", async () => {
    loadConversation.mockResolvedValue({
      sessionId: "s1",
      turns: [turn("u1", "first turn"), turn("u2", "second turn")],
      offset: 100,
    });
    render(wrap(<ConversationPane sessionId="s1" />));
    await waitFor(() => expect(screen.getByText("first turn")).toBeDefined());
    expect(screen.getByText("second turn")).toBeDefined();
  });

  it("shows an empty state for a conversation with no turns", async () => {
    loadConversation.mockResolvedValue({ sessionId: "s1", turns: [], offset: 0 });
    render(wrap(<ConversationPane sessionId="s1" />));
    await waitFor(() => expect(screen.getByText(/nothing to show/i)).toBeDefined());
  });

  it("surfaces a load error", async () => {
    loadConversation.mockRejectedValue(new Error("no transcript found"));
    render(wrap(<ConversationPane sessionId="s1" />));
    await waitFor(() => expect(screen.getByRole("alert")).toBeDefined());
  });

  it("appends turns delivered by a poll", async () => {
    loadConversation.mockResolvedValue({ sessionId: "s1", turns: [turn("u1", "first")], offset: 10 });
    pollConversation.mockResolvedValue({ turns: [turn("u2", "appended")], updates: [], offset: 20, reset: false });
    render(wrap(<ConversationPane sessionId="s1" />));
    await waitFor(() => expect(screen.getByText("appended")).toBeDefined(), { timeout: 3000 });
    expect(screen.getByText("first")).toBeDefined();
  });

  it("replaces everything when a poll signals reset", async () => {
    loadConversation.mockResolvedValue({ sessionId: "s1", turns: [turn("u1", "stale")], offset: 10 });
    pollConversation.mockResolvedValue({ turns: [turn("u9", "fresh")], updates: [], offset: 5, reset: true });
    render(wrap(<ConversationPane sessionId="s1" />));
    await waitFor(() => expect(screen.getByText("fresh")).toBeDefined(), { timeout: 3000 });
    expect(screen.queryByText("stale")).toBeNull();
  });

  const withAgent = (agentId: string): Turn => ({
    uuid: "u1",
    role: "assistant",
    timestamp: "2026-07-31T00:00:00Z",
    blocks: [
      {
        kind: "tool",
        call: {
          id: "t1",
          name: "Agent",
          description: "Review the diff",
          result: "done",
          isError: false,
          agentId,
        },
      },
    ],
    model: null,
    usage: null,
  });

  it("does not load a subagent until the user asks for it", async () => {
    loadConversation.mockResolvedValue({ sessionId: "s1", turns: [withAgent("a1")], offset: 10 });
    loadSubagent.mockResolvedValue({ sessionId: "s1", turns: [turn("s1", "nested")], offset: 0 });

    render(wrap(<ConversationPane sessionId="s1" />));
    await waitFor(() => expect(screen.getByText("Review the diff")).toBeDefined());

    // Lazy: one session had 48 subagent files, so eager loading would be slow.
    expect(loadSubagent).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: /subagent/i }));
    await waitFor(() => expect(loadSubagent).toHaveBeenCalledWith("s1", "a1"));
    expect(await screen.findByText("nested")).toBeDefined();
  });

  it("says a subagent transcript is unavailable rather than breaking the turn", async () => {
    loadConversation.mockResolvedValue({ sessionId: "s1", turns: [withAgent("missing")], offset: 10 });
    // 3 of 48 real subagent files did not link cleanly, so this is a real case.
    loadSubagent.mockRejectedValue(new Error("subagent transcript unavailable"));

    render(wrap(<ConversationPane sessionId="s1" />));
    await waitFor(() => expect(screen.getByText("Review the diff")).toBeDefined());
    fireEvent.click(screen.getByRole("button", { name: /subagent/i }));

    expect(await screen.findByRole("alert")).toBeDefined();
    // The parent turn must still be there.
    expect(screen.getByText("Review the diff")).toBeDefined();
  });

  it("renders two turns that share a uuid", async () => {
    // Measured: 7 of 1339 real transcripts repeat a uuid across two lines.
    const dup = (text: string): Turn => ({ ...turn("same-uuid", text) });
    loadConversation.mockResolvedValue({
      sessionId: "s1",
      turns: [dup("first copy"), dup("second copy")],
      offset: 10,
    });
    render(wrap(<ConversationPane sessionId="s1" />));
    await waitFor(() => expect(screen.getByText("first copy")).toBeDefined());
    expect(screen.getByText("second copy")).toBeDefined();
  });

  it("stops polling and says so after repeated read failures", async () => {
    loadConversation.mockResolvedValue({ sessionId: "s1", turns: [turn("u1", "content")], offset: 10 });
    pollConversation.mockRejectedValue(new Error("could not read transcript"));
    render(wrap(<ConversationPane sessionId="s1" />));
    await waitFor(() => expect(screen.getByText("content")).toBeDefined());

    // Five consecutive failures at ~1s each; allow generous headroom. The
    // vitest default 5000ms per-test timeout is shorter than that, so it is
    // raised here rather than lowering MAX_CONSECUTIVE_FAILURES in the source.
    expect(await screen.findByRole("alert", {}, { timeout: 10000 })).toBeDefined();
  }, 15000);

  it("closes an expanded subagent when the selected session changes", async () => {
    // Both sessions contain a call with the SAME agentId, so the render guard
    // (`turns.some(block.call.agentId === openAgent)`) is satisfied before and
    // after the switch. That isolates `openAgent` as the only variable: if the
    // sessionId effect fails to clear it, the panel stays open.
    loadConversation.mockResolvedValue({
      sessionId: "s1",
      turns: [withAgent("a1")],
      offset: 10,
    });
    loadSubagent.mockResolvedValue({
      sessionId: "s1",
      turns: [turn("sub", "nested content")],
      offset: 0,
    });

    const { rerender } = render(wrap(<ConversationPane sessionId="s1" />));
    await waitFor(() => expect(screen.getByText("Review the diff")).toBeDefined());

    fireEvent.click(screen.getByRole("button", { name: /subagent/i }));
    expect(await screen.findByText("nested content")).toBeDefined();

    // The new session ALSO has a call carrying agentId "a1" -- so a stale
    // openAgent would still satisfy the render guard and keep the panel open.
    const s2Turn: Turn = { ...withAgent("a1"), uuid: "u2" };
    loadConversation.mockResolvedValue({
      sessionId: "s2",
      turns: [s2Turn],
      offset: 10,
    });
    rerender(wrap(<ConversationPane sessionId="s2" />));

    // Wait for the new session's data to land before asserting.
    await waitFor(() => expect(loadConversation).toHaveBeenCalledWith("s2"));
    await waitFor(() => expect(screen.queryByText("nested content")).toBeNull());
  });
});

describe("applyDelta", () => {
  const withCall = (uuid: string, id: string, result: string | null): Turn => ({
    uuid,
    role: "assistant",
    timestamp: "2026-07-31T00:00:00Z",
    blocks: [
      {
        kind: "tool",
        call: { id, name: "Bash", description: "Run tests", result, isError: false, agentId: null },
      },
    ],
    model: null,
    usage: null,
  });

  it("appends new turns when there are no updates", () => {
    const out = applyDelta([turn("u1", "first")], [turn("u2", "second")], []);
    expect(out).toHaveLength(2);
  });

  it("patches a late result into a turn already held", () => {
    // The measured common case: the call arrived in an earlier poll.
    const out = applyDelta(
      [withCall("u1", "t1", null)],
      [],
      [{ toolUseId: "t1", result: "all green", isError: false }],
    );
    expect(out).toHaveLength(1);
    const b = out[0].blocks[0];
    expect(b.kind === "tool" && b.call.result).toBe("all green");
  });

  it("carries the error flag through a patch", () => {
    const out = applyDelta(
      [withCall("u1", "t1", null)],
      [],
      [{ toolUseId: "t1", result: "boom", isError: true }],
    );
    const b = out[0].blocks[0];
    expect(b.kind === "tool" && b.call.isError).toBe(true);
  });

  it("leaves unrelated turns untouched by identity", () => {
    const keep = turn("u1", "prose");
    const out = applyDelta(
      [keep, withCall("u2", "t1", null)],
      [],
      [{ toolUseId: "t1", result: "done", isError: false }],
    );
    expect(out[0]).toBe(keep);
  });

  it("ignores an update for a call it does not hold", () => {
    const prev = [turn("u1", "prose")];
    const out = applyDelta(prev, [], [{ toolUseId: "nope", result: "x", isError: false }]);
    expect(out).toEqual(prev);
  });
});
