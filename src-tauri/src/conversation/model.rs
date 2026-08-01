use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    User,
    Assistant,
}

/// Token usage for one assistant turn, as reported in `message.usage`.
///
/// The wire format is snake_case (`input_tokens`), but the TypeScript
/// boundary needs camelCase. `rename_all` governs BOTH directions, so each
/// field carries an explicit `alias` for the snake_case name it is actually
/// read from. Without the aliases every field silently deserializes to 0 --
/// `serde(default)` turns the missing-key error into a zero.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    #[serde(default, alias = "input_tokens")]
    pub input_tokens: u64,
    #[serde(default, alias = "output_tokens")]
    pub output_tokens: u64,
    #[serde(default, alias = "cache_read_input_tokens")]
    pub cache_read_input_tokens: u64,
}

/// One tool invocation and, when present, its result.
///
/// `result` is None for an orphaned call -- a session killed mid-tool-call
/// leaves a `tool_use` with no matching `tool_result`. Measured at 0 of 10947
/// in real transcripts, but it is a real state and must render.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// `input.description` when present -- a human-readable label Claude Code
    /// already writes. Falls back to the tool name in the UI.
    pub description: Option<String>,
    pub result: Option<String>,
    pub is_error: bool,
    /// Set when this call spawned a subagent whose transcript can be loaded.
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Block {
    Text { text: String },
    Tool { call: ToolCall },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub uuid: String,
    pub role: Role,
    pub timestamp: String,
    pub blocks: Vec<Block>,
    pub model: Option<String>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub session_id: String,
    pub turns: Vec<Turn>,
    /// Byte offset to resume tailing from.
    pub offset: u64,
}

/// A late-arriving result for a tool call the client has ALREADY rendered.
///
/// Load-bearing, not an optimisation. Measured on real transcripts: 60% of
/// tool calls (6726 of 11170) take longer than a second between `tool_use` and
/// `tool_result`, and the conversation polls at 1s. So for most calls on a live
/// session the result lands in a LATER poll than the call. Without a patch
/// channel those results are dropped and the call renders "no result" forever,
/// indistinguishable from a genuinely orphaned call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultUpdate {
    /// The `tool_use` id whose call should be patched.
    pub tool_use_id: String,
    pub result: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDelta {
    pub turns: Vec<Turn>,
    /// Results for calls emitted in an earlier poll. The client patches these
    /// into turns it already holds.
    pub updates: Vec<ToolResultUpdate>,
    pub offset: u64,
    /// True when the file was truncated or replaced and the client must
    /// discard what it has and re-render from `turns`.
    pub reset: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_serializes_to_camel_case() {
        let t = Turn {
            uuid: "u1".into(),
            role: Role::Assistant,
            timestamp: "2026-07-31T00:00:00Z".into(),
            blocks: vec![Block::Text { text: "hi".into() }],
            model: Some("claude-opus-5".into()),
            usage: None,
        };
        let j = serde_json::to_string(&t).unwrap();
        assert!(j.contains("\"role\":\"assistant\""));
        assert!(j.contains("\"kind\":\"text\""));
        assert!(j.contains("\"model\":\"claude-opus-5\""));
        // Option fields must be explicit null, never omitted.
        assert!(j.contains("\"usage\":null"));
    }

    #[test]
    fn tool_block_serializes_with_its_call() {
        let b = Block::Tool {
            call: ToolCall {
                id: "toolu_1".into(),
                name: "Bash".into(),
                description: Some("List files".into()),
                result: Some("total 0".into()),
                is_error: false,
                agent_id: None,
            },
        };
        let j = serde_json::to_string(&b).unwrap();
        assert!(j.contains("\"kind\":\"tool\""));
        assert!(j.contains("\"name\":\"Bash\""));
        assert!(j.contains("\"isError\":false"));
        assert!(j.contains("\"agentId\":null"));
    }

    #[test]
    fn delta_carries_a_reset_flag() {
        let d = ConversationDelta {
            turns: vec![],
            updates: vec![],
            offset: 42,
            reset: true,
        };
        let j = serde_json::to_string(&d).unwrap();
        assert!(j.contains("\"reset\":true"));
        assert!(j.contains("\"offset\":42"));
    }

    #[test]
    fn usage_deserializes_the_snake_case_wire_format() {
        // Transcripts write snake_case; rename_all would otherwise make
        // deserialization expect camelCase and silently yield zeros.
        let real = r#"{"input_tokens":5,"output_tokens":7,"cache_read_input_tokens":9}"#;
        let u: Usage = serde_json::from_str(real).unwrap();
        assert_eq!(u.input_tokens, 5);
        assert_eq!(u.output_tokens, 7);
        assert_eq!(u.cache_read_input_tokens, 9);
    }

    #[test]
    fn usage_still_serializes_camel_case_for_typescript() {
        let u = Usage {
            input_tokens: 1,
            output_tokens: 2,
            cache_read_input_tokens: 3,
        };
        let j = serde_json::to_string(&u).unwrap();
        assert!(j.contains("\"inputTokens\":1"), "got {j}");
        assert!(j.contains("\"outputTokens\":2"), "got {j}");
    }

    #[test]
    fn usage_tolerates_the_extra_fields_real_transcripts_carry() {
        // Real usage objects also carry cache_creation_input_tokens,
        // server_tool_use, service_tier. Serde ignores unknown keys.
        let real = r#"{"input_tokens":2,"cache_creation_input_tokens":27275,"output_tokens":153,"server_tool_use":{"web_search_requests":0}}"#;
        let u: Usage = serde_json::from_str(real).unwrap();
        assert_eq!(u.input_tokens, 2);
        assert_eq!(u.output_tokens, 153);
    }
}
