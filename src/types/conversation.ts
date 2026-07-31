export type Role = "user" | "assistant";

export interface Usage {
  inputTokens: number;
  outputTokens: number;
  cacheReadInputTokens: number;
}

export interface ToolCall {
  id: string;
  name: string;
  description: string | null;
  result: string | null;
  isError: boolean;
  agentId: string | null;
}

export type Block =
  | { kind: "text"; text: string }
  | { kind: "tool"; call: ToolCall };

export interface Turn {
  uuid: string;
  role: Role;
  timestamp: string;
  blocks: Block[];
  model: string | null;
  usage: Usage | null;
}

export interface Conversation {
  sessionId: string;
  turns: Turn[];
  offset: number;
}

/// A result for a tool call the client has already rendered. Measured: 60% of
/// real tool calls outlast the 1s poll interval, so their result arrives in a
/// later poll than the call and must be patched in, not appended.
export interface ToolResultUpdate {
  toolUseId: string;
  result: string;
  isError: boolean;
}

export interface ConversationDelta {
  turns: Turn[];
  updates: ToolResultUpdate[];
  offset: number;
  reset: boolean;
}
