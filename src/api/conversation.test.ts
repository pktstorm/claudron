import { describe, it, expect, vi, beforeEach } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import { loadConversation, pollConversation, loadSubagent } from "./conversation";

describe("conversation api", () => {
  beforeEach(() => invoke.mockReset());

  it("loadConversation passes the session id", async () => {
    invoke.mockResolvedValue({ sessionId: "s1", turns: [], offset: 0 });
    await loadConversation("s1");
    expect(invoke).toHaveBeenCalledWith("load_conversation", { sessionId: "s1" });
  });

  it("pollConversation passes the session id and offset", async () => {
    invoke.mockResolvedValue({ turns: [], offset: 10, reset: false });
    await pollConversation("s1", 5);
    expect(invoke).toHaveBeenCalledWith("poll_conversation", { sessionId: "s1", offset: 5 });
  });

  it("loadSubagent passes the session id and agent id", async () => {
    invoke.mockResolvedValue({ sessionId: "s1", turns: [], offset: 0 });
    await loadSubagent("s1", "a1");
    expect(invoke).toHaveBeenCalledWith("load_subagent", { sessionId: "s1", agentId: "a1" });
  });
});
