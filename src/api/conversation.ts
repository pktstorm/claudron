import { invoke } from "@tauri-apps/api/core";
import type { Conversation, ConversationDelta } from "../types/conversation";

export function loadConversation(sessionId: string): Promise<Conversation> {
  return invoke("load_conversation", { sessionId });
}

export function pollConversation(
  sessionId: string,
  offset: number,
): Promise<ConversationDelta> {
  return invoke("poll_conversation", { sessionId, offset });
}

export function loadSubagent(sessionId: string, agentId: string): Promise<Conversation> {
  return invoke("load_subagent", { sessionId, agentId });
}
