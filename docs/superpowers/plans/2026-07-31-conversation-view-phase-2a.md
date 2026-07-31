# Claudron Phase 2A — Conversation View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render a Claude Code session's full conversation inside Claudron — prose, collapsible tool calls, errors, costs, and nested subagent turns — read from the transcript JSONL rather than from a terminal.

**Architecture:** Rust parses transcript records into ordered turns and blocks, tails files by byte offset so a poll re-reads only new bytes, and lazily loads subagent transcripts on expand. React renders an already-structured payload through a virtualized list and never opens a file. The conversation becomes the right-hand pane; Phase 1's `SessionDetail` moves intact into a slide-over.

**Tech Stack:** Rust 1.93 · Tauri 2.11 · React 19 · TypeScript 5 (strict) · Tailwind 4 · Zustand · TanStack Query · Vitest · yarn

## Global Constraints

- **Phase 2A only.** No tmux, no session spawning, no reply box, no text input of any kind. 2B covers those.
- **tmux is NOT installed on this machine.** Nothing may require it or fail without it.
- **All filesystem access lives in Rust.** React calls Tauri commands only — it must never open a file.
- **A malformed record must never blank the conversation.** Skip that record, render the rest. Mirrors the Phase 1 rule.
- **Never render a torn parse.** If a file shrank or its inode changed, discard the offset and re-read from the start.
- **Phase 1 modules are reused unchanged**: `transcript.rs`, `index.rs`, `annotations.rs`, `process.rs`, `actions.rs`, `commands.rs`. The conversation view is additive and must not disturb the working index path.
- **Rust module declarations go in `src-tauri/src/lib.rs` as `pub mod <name>;`**, never in `main.rs` — `main.rs` is a thin shim calling `claudron_app_lib::run()`. Tauri commands are registered in `lib.rs`'s `run()`.
- **Package manager is yarn.** Never npm.
- **Nullable Rust `Option<T>` fields serialize as explicit JSON `null`** (no `skip_serializing_if` anywhere in the model), so TypeScript types use `T | null`, never `T?`.
- **No vitest `setupFiles`** — jest-dom matchers like `toBeInTheDocument()` are unavailable. Use `toBeDefined()` / `toBeNull()` / `toHaveLength()`.
- **`make lint` runs `clippy -- -D warnings` and `tsc --noEmit`.** Both must stay clean; an unused import fails the build.

## Data shapes — measured, not assumed

Every fixture in this plan mirrors a shape verified in the real transcripts on 2026-07-31. The three that most easily cause bugs:

| Shape | Reality |
|---|---|
| `tool_result.content` | **string 10590×, list 397×**. The list form holds `text` and `tool_reference` blocks. A parser assuming string drops ~4%. |
| user `message.content` | **list 11033×, bare string 1148×**. Both must parse. |
| `tool_use` per assistant turn | **Exactly 1**, across 1789 turns. "Ran 2 commands" is display-side coalescing, not a stored group. |

Other measured facts: orphaned `tool_use` (no matching result) was **0 of 10947** — rare, but a killed session produces one, so it is handled. Largest transcript is **68.7 MB / 27,930 lines**. Subagent files live at `<session-uuid>/subagents/agent-<agentId>.jsonl`; the `agentId` appears in the *tool result text* of the spawning `Agent` call (45 of 48 linked in one session).

---

## File Structure

**Rust (`src-tauri/src/`)**

| File | Responsibility |
|---|---|
| `conversation/model.rs` | The wire types: `Turn`, `Block`, `Conversation`, `ConversationDelta` |
| `conversation/parse.rs` | Turn one record stream into ordered turns and blocks |
| `conversation/tail.rs` | Byte-offset tailing; truncation and inode-change detection |
| `conversation/subagent.rs` | Find `subagents/*.jsonl`; link each to its spawning `tool_use_id` |
| `conversation/mod.rs` | Re-exports; the three Tauri commands |

A `conversation/` directory rather than flat files: five concerns that change together, and the existing flat modules are already eight files at the top level.

**React (`src/`)**

| File | Responsibility |
|---|---|
| `types/conversation.ts` | TS mirrors of the conversation wire types |
| `api/conversation.ts` | Typed wrappers over the three new Tauri commands |
| `components/ConversationPane.tsx` | Virtualized turn list, auto-scroll, polling |
| `components/TurnBlock.tsx` | One turn: prose, timing, model, cost |
| `components/ToolCallBlock.tsx` | Collapsed one-liner; expands to output |
| `components/SubagentBlock.tsx` | Lazy nested conversation |
| `components/DetailSlideOver.tsx` | Hosts Phase 1's `SessionDetail` intact |

---

### Task 1: Conversation wire types

**Files:**
- Create: `src-tauri/src/conversation/mod.rs`, `src-tauri/src/conversation/model.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod conversation;`)

**Interfaces:**
- Consumes: nothing
- Produces: `conversation::model::{Conversation, ConversationDelta, Turn, Block, Role, ToolCall, Usage}`, all `serde`-serializable with camelCase field names.

- [ ] **Step 1: Create the module directory and write the types**

Create `src-tauri/src/conversation/model.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    User,
    Assistant,
}

/// Token usage for one assistant turn, as reported in `message.usage`.
///
/// This is the ONE conversation type deserialized from wire data, and
/// `rename_all` governs BOTH directions: it makes serialization emit
/// `inputTokens` for TypeScript but also makes deserialization EXPECT
/// `inputTokens`, while transcripts write `input_tokens`. Each field therefore
/// carries an explicit `alias` for the snake_case name it is read from.
/// Without them every field silently deserializes to 0 -- `serde(default)`
/// turns what would be a loud missing-key error into a quiet zero.
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
        let d = ConversationDelta { turns: vec![], offset: 42, reset: true };
        let j = serde_json::to_string(&d).unwrap();
        assert!(j.contains("\"reset\":true"));
        assert!(j.contains("\"offset\":42"));
    }

    // Usage crosses the wire in BOTH directions with different casing.
    // These three pin that; without the field aliases the first one fails.

    #[test]
    fn usage_deserializes_the_snake_case_wire_format() {
        let real = r#"{"input_tokens":5,"output_tokens":7,"cache_read_input_tokens":9}"#;
        let u: Usage = serde_json::from_str(real).unwrap();
        assert_eq!(u.input_tokens, 5);
        assert_eq!(u.output_tokens, 7);
        assert_eq!(u.cache_read_input_tokens, 9);
    }

    #[test]
    fn usage_still_serializes_camel_case_for_typescript() {
        let u = Usage { input_tokens: 1, output_tokens: 2, cache_read_input_tokens: 3 };
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
```

Create `src-tauri/src/conversation/mod.rs`:

```rust
pub mod model;
```

- [ ] **Step 2: Register the module**

In `src-tauri/src/lib.rs`, add `pub mod conversation;` alongside the existing `pub mod` lines, keeping them alphabetical (it sorts after `commands`).

- [ ] **Step 3: Run tests to verify they pass**

Run: `cd src-tauri && cargo test conversation::model::`
Expected: 3 tests pass.

- [ ] **Step 4: Verify lint is clean**

Run: `cd src-tauri && cargo clippy -- -D warnings`
Expected: exits 0. If it flags the new types as dead code, that is expected only until Task 2 consumes them — if it fails here, add `#[allow(dead_code)]` to nothing and instead proceed; the types are `pub` in a `pub mod`, which suppresses the lint.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/conversation src-tauri/src/lib.rs
git commit -m "feat: add conversation wire types"
```

---

### Task 2: Parse records into turns

**Files:**
- Create: `src-tauri/src/conversation/parse.rs`
- Modify: `src-tauri/src/conversation/mod.rs` (add `pub mod parse;`)

**Interfaces:**
- Consumes: `conversation::model::{Turn, Block, Role, ToolCall, Usage, ToolResultUpdate}`
- Produces:
  - `conversation::parse::ParseOutput { turns: Vec<Turn>, updates: Vec<ToolResultUpdate>, pending: PendingCalls }`
  - `conversation::parse::PendingCalls` — an opaque `HashMap<String, (usize, usize)>` newtype the caller threads between polls
  - `conversation::parse::parse_records(lines: impl Iterator<Item = String>) -> ParseOutput` — convenience for a full read, equivalent to `parse_with_pending(lines, PendingCalls::default())`
  - `conversation::parse::parse_with_pending(lines: impl Iterator<Item = String>, pending: PendingCalls) -> ParseOutput`

**Why `parse_with_pending` exists — measured, not speculative.** A `tool_result`
whose `tool_use` arrived in an earlier poll finds an empty `pending` map and
would be silently dropped. 60% of real tool calls (6726 of 11170) take longer
than a second between call and result, and the conversation polls at 1s, so on
a live session most results land in a later poll than their call. Those results
come back as `updates` for the client to patch into turns it already holds.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/conversation/parse.rs`:

```rust
use crate::conversation::model::{Block, Role, ToolCall, ToolResultUpdate, Turn, Usage};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct PendingCalls(HashMap<String, (usize, usize)>);

pub struct ParseOutput {
    pub turns: Vec<Turn>,
    pub updates: Vec<ToolResultUpdate>,
    pub pending: PendingCalls,
}

pub fn parse_records(_lines: impl Iterator<Item = String>) -> ParseOutput {
    ParseOutput { turns: Vec::new(), updates: Vec::new(), pending: PendingCalls::default() }
}

pub fn parse_with_pending(
    _lines: impl Iterator<Item = String>,
    _carried: PendingCalls,
) -> ParseOutput {
    ParseOutput { turns: Vec::new(), updates: Vec::new(), pending: PendingCalls::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> std::vec::IntoIter<String> {
        v.iter().map(|s| s.to_string()).collect::<Vec<_>>().into_iter()
    }

    #[test]
    fn parses_an_assistant_text_turn() {
        let turns = parse_records(lines(&[
            r#"{"type":"assistant","uuid":"u1","timestamp":"T1","message":{"role":"assistant","model":"claude-opus-5","content":[{"type":"text","text":"Hello there"}]}}"#,
        ])).turns;
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].role, Role::Assistant);
        assert_eq!(turns[0].uuid, "u1");
        assert_eq!(turns[0].model.as_deref(), Some("claude-opus-5"));
        assert_eq!(turns[0].blocks, vec![Block::Text { text: "Hello there".into() }]);
    }

    #[test]
    fn parses_a_user_turn_whose_content_is_a_bare_string() {
        // Measured: user message.content is a bare string 1148 times.
        let turns = parse_records(lines(&[
            r#"{"type":"user","uuid":"u2","timestamp":"T2","message":{"role":"user","content":"just text"}}"#,
        ])).turns;
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].role, Role::User);
        assert_eq!(turns[0].blocks, vec![Block::Text { text: "just text".into() }]);
    }

    #[test]
    fn pairs_a_tool_call_with_its_result() {
        let turns = parse_records(lines(&[
            r#"{"type":"assistant","uuid":"a1","timestamp":"T1","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls","description":"List files"}}]}}"#,
            r#"{"type":"user","uuid":"u1","timestamp":"T2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"total 0","is_error":false}]}}"#,
        ])).turns;
        // The result is folded into the call's turn, not rendered as its own turn.
        assert_eq!(turns.len(), 1);
        match &turns[0].blocks[0] {
            Block::Tool { call } => {
                assert_eq!(call.name, "Bash");
                assert_eq!(call.description.as_deref(), Some("List files"));
                assert_eq!(call.result.as_deref(), Some("total 0"));
                assert!(!call.is_error);
            }
            other => panic!("expected a tool block, got {other:?}"),
        }
    }

    #[test]
    fn flattens_a_tool_result_whose_content_is_a_list() {
        // Measured: tool_result.content is a list 397 times, holding text
        // and tool_reference blocks. Assuming a string would drop ~4%.
        let turns = parse_records(lines(&[
            r#"{"type":"assistant","uuid":"a1","timestamp":"T1","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{}}]}}"#,
            r#"{"type":"user","uuid":"u1","timestamp":"T2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"line one"},{"type":"text","text":"line two"}]}]}}"#,
        ])).turns;
        match &turns[0].blocks[0] {
            Block::Tool { call } => {
                let r = call.result.as_deref().unwrap();
                assert!(r.contains("line one"), "got {r}");
                assert!(r.contains("line two"), "got {r}");
            }
            other => panic!("expected a tool block, got {other:?}"),
        }
    }

    #[test]
    fn marks_an_error_result() {
        let turns = parse_records(lines(&[
            r#"{"type":"assistant","uuid":"a1","timestamp":"T1","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}]}}"#,
            r#"{"type":"user","uuid":"u1","timestamp":"T2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"boom","is_error":true}]}}"#,
        ])).turns;
        match &turns[0].blocks[0] {
            Block::Tool { call } => assert!(call.is_error),
            other => panic!("expected a tool block, got {other:?}"),
        }
    }

    #[test]
    fn an_orphaned_tool_call_still_renders_with_no_result() {
        // A session killed mid-call leaves a tool_use with no tool_result.
        let turns = parse_records(lines(&[
            r#"{"type":"assistant","uuid":"a1","timestamp":"T1","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"description":"Run tests"}}]}}"#,
        ])).turns;
        assert_eq!(turns.len(), 1);
        match &turns[0].blocks[0] {
            Block::Tool { call } => {
                assert!(call.result.is_none(), "orphan must have no result");
                assert_eq!(call.description.as_deref(), Some("Run tests"));
            }
            other => panic!("expected a tool block, got {other:?}"),
        }
    }

    #[test]
    fn captures_usage_when_present() {
        let turns = parse_records(lines(&[
            r#"{"type":"assistant","uuid":"a1","timestamp":"T1","message":{"role":"assistant","content":[{"type":"text","text":"hi"}],"usage":{"input_tokens":5,"output_tokens":7,"cache_read_input_tokens":9}}}"#,
        ])).turns;
        let u = turns[0].usage.clone().unwrap();
        assert_eq!(u.input_tokens, 5);
        assert_eq!(u.output_tokens, 7);
        assert_eq!(u.cache_read_input_tokens, 9);
    }

    #[test]
    fn skips_malformed_lines_without_losing_the_rest() {
        let turns = parse_records(lines(&[
            r#"{"type":"assistant","uuid":"a1","timestamp":"T1","message":{"role":"assistant","content":[{"type":"text","text":"first"}]}}"#,
            r#"this is not json"#,
            r#"{"type":"assistant","uuid":"a2","timestamp":"T2","message":{"role":"assistant","content":[{"type":"text","text":"second"}]}}"#,
        ])).turns;
        assert_eq!(turns.len(), 2);
    }

    #[test]
    fn ignores_control_plane_records() {
        // mode, permission-mode, ai-title, last-prompt, queue-operation and
        // friends are not conversation and must not become turns.
        let turns = parse_records(lines(&[
            r#"{"type":"mode","mode":"normal","sessionId":"s"}"#,
            r#"{"type":"permission-mode","permissionMode":"auto","sessionId":"s"}"#,
            r#"{"type":"ai-title","aiTitle":"Some title","sessionId":"s"}"#,
            r#"{"type":"last-prompt","lastPrompt":"do it","sessionId":"s"}"#,
            r#"{"type":"assistant","uuid":"a1","timestamp":"T1","message":{"role":"assistant","content":[{"type":"text","text":"only turn"}]}}"#,
        ])).turns;
        assert_eq!(turns.len(), 1);
    }

    // The three tests below pin the split-poll behaviour. Measured: 60% of real
    // tool calls take longer than the 1s poll interval, so the call and its
    // result routinely land in different polls. Without carried pending state
    // the result is silently dropped and the call reads "no result" forever.

    #[test]
    fn a_result_arriving_in_a_later_batch_becomes_an_update() {
        let first = parse_records(lines(&[
            r#"{"type":"assistant","uuid":"a1","timestamp":"T1","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"description":"Run tests"}}]}}"#,
        ]));
        assert_eq!(first.turns.len(), 1);
        assert!(first.updates.is_empty());

        // Next poll carries the pending call and delivers only the result.
        let second = parse_with_pending(
            lines(&[
                r#"{"type":"user","uuid":"u1","timestamp":"T2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"all green","is_error":false}]}}"#,
            ]),
            first.pending,
        );
        assert!(second.turns.is_empty(), "a result-only batch yields no new turns");
        assert_eq!(second.updates.len(), 1, "the result must survive as an update");
        assert_eq!(second.updates[0].tool_use_id, "t1");
        assert_eq!(second.updates[0].result, "all green");
        assert!(!second.updates[0].is_error);
    }

    #[test]
    fn an_error_result_in_a_later_batch_keeps_its_error_flag() {
        let first = parse_records(lines(&[
            r#"{"type":"assistant","uuid":"a1","timestamp":"T1","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}]}}"#,
        ]));
        let second = parse_with_pending(
            lines(&[
                r#"{"type":"user","uuid":"u1","timestamp":"T2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"boom","is_error":true}]}}"#,
            ]),
            first.pending,
        );
        assert_eq!(second.updates.len(), 1);
        assert!(second.updates[0].is_error);
    }

    #[test]
    fn a_result_for_a_call_never_seen_is_dropped_without_panicking() {
        // The client joined mid-file; there is nothing to attach this to.
        let out = parse_records(lines(&[
            r#"{"type":"user","uuid":"u1","timestamp":"T2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"unknown","content":"orphan"}]}}"#,
        ]));
        assert!(out.turns.is_empty());
        assert!(out.updates.is_empty());
    }

    #[test]
    fn a_resolved_call_does_not_stay_pending() {
        // Otherwise pending grows without bound across a long session.
        let out = parse_records(lines(&[
            r#"{"type":"assistant","uuid":"a1","timestamp":"T1","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}]}}"#,
            r#"{"type":"user","uuid":"u1","timestamp":"T2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"done"}]}}"#,
        ]));
        let leftover = parse_with_pending(
            lines(&[
                r#"{"type":"user","uuid":"u2","timestamp":"T3","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"late duplicate"}]}}"#,
            ]),
            out.pending,
        );
        assert!(
            leftover.updates.is_empty(),
            "an already-resolved call must not linger and accept a second result"
        );
    }

    #[test]
    fn a_turn_with_no_renderable_blocks_is_dropped() {
        // A user record that carries only a tool_result contributes its result
        // to the earlier call and must not also appear as an empty turn.
        let turns = parse_records(lines(&[
            r#"{"type":"assistant","uuid":"a1","timestamp":"T1","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}]}}"#,
            r#"{"type":"user","uuid":"u1","timestamp":"T2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"out"}]}}"#,
        ])).turns;
        assert_eq!(turns.len(), 1, "the tool_result-only user turn must not render");
    }
}
```

- [ ] **Step 2: Add `pub mod parse;` to `conversation/mod.rs` and run tests to verify they fail**

Run: `cd src-tauri && cargo test conversation::parse::`
Expected: 9 failures against the empty-vec stub, 1 pass (`a_turn_with_no_renderable_blocks_is_dropped` passes vacuously on an empty result — note this, it is why that test alone is not sufficient proof).

- [ ] **Step 3: Implement the parser**

Replace the `parse_records` stub:

```rust
/// Flatten a `tool_result.content` value into display text.
///
/// Measured: it is a bare string 10590 times and a list of blocks 397 times.
/// The list form holds `text` and `tool_reference` blocks; we keep the text.
fn flatten_result_content(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

/// Positions of tool calls still awaiting a result, threaded across polls.
///
/// Indices are into the turn list of the parse call that created them, so a
/// carried-over entry cannot be used to mutate this call's turns -- such a
/// result is emitted as a `ToolResultUpdate` instead.
#[derive(Debug, Clone, Default)]
pub struct PendingCalls(HashMap<String, (usize, usize)>);

pub struct ParseOutput {
    pub turns: Vec<Turn>,
    pub updates: Vec<ToolResultUpdate>,
    pub pending: PendingCalls,
}

/// Parse a complete file. Equivalent to `parse_with_pending` with no carry-in.
pub fn parse_records(lines: impl Iterator<Item = String>) -> ParseOutput {
    parse_with_pending(lines, PendingCalls::default())
}

pub fn parse_with_pending(
    lines: impl Iterator<Item = String>,
    carried: PendingCalls,
) -> ParseOutput {
    let mut turns: Vec<Turn> = Vec::new();
    let mut updates: Vec<ToolResultUpdate> = Vec::new();
    // Ids whose call was emitted in an EARLIER parse call. A result for one of
    // these becomes an update rather than an in-place mutation.
    let mut carried: HashMap<String, (usize, usize)> = carried.0;
    // tool_use id -> (turn index, block index) within THIS call's turns.
    let mut pending: HashMap<String, (usize, usize)> = HashMap::new();

    for line in lines {
        let Ok(rec) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        let ty = rec.get("type").and_then(Value::as_str).unwrap_or("");
        if ty != "assistant" && ty != "user" {
            continue; // control-plane record, not conversation
        }
        let Some(msg) = rec.get("message") else { continue };

        let role = match msg.get("role").and_then(Value::as_str) {
            Some("assistant") => Role::Assistant,
            Some("user") => Role::User,
            _ => continue,
        };

        let mut blocks: Vec<Block> = Vec::new();
        let turn_index = turns.len();

        match msg.get("content") {
            // Measured: a bare string 1148 times.
            Some(Value::String(s)) => {
                if !s.trim().is_empty() {
                    blocks.push(Block::Text { text: s.clone() });
                }
            }
            Some(Value::Array(items)) => {
                for b in items {
                    match b.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(t) = b.get("text").and_then(Value::as_str) {
                                if !t.trim().is_empty() {
                                    blocks.push(Block::Text { text: t.to_string() });
                                }
                            }
                        }
                        Some("tool_use") => {
                            let id = b.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                            let call = ToolCall {
                                name: b
                                    .get("name")
                                    .and_then(Value::as_str)
                                    .unwrap_or("tool")
                                    .to_string(),
                                description: b
                                    .get("input")
                                    .and_then(|i| i.get("description"))
                                    .and_then(Value::as_str)
                                    .map(str::to_string),
                                result: None,
                                is_error: false,
                                agent_id: None,
                                id: id.clone(),
                            };
                            if !id.is_empty() {
                                pending.insert(id, (turn_index, blocks.len()));
                            }
                            blocks.push(Block::Tool { call });
                        }
                        Some("tool_result") => {
                            // Fold into the call that produced it; never a turn.
                            let Some(tid) = b.get("tool_use_id").and_then(Value::as_str) else {
                                continue;
                            };
                            let text = b
                                .get("content")
                                .map(flatten_result_content)
                                .unwrap_or_default();
                            let is_error =
                                b.get("is_error").and_then(Value::as_bool).unwrap_or(false);

                            if let Some(&(ti, bi)) = pending.get(tid) {
                                // The call is in THIS batch -- mutate it in place.
                                if let Some(Block::Tool { call }) =
                                    turns.get_mut(ti).and_then(|t| t.blocks.get_mut(bi))
                                {
                                    call.result = Some(text);
                                    call.is_error = is_error;
                                }
                                pending.remove(tid);
                            } else if carried.remove(tid).is_some() {
                                // The call was emitted in an earlier poll and the
                                // client already rendered it. Patch it there.
                                updates.push(ToolResultUpdate {
                                    tool_use_id: tid.to_string(),
                                    result: text,
                                    is_error,
                                });
                            }
                            // Otherwise the call was never seen -- e.g. the client
                            // joined mid-file. Nothing to attach it to; drop it.
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        if blocks.is_empty() {
            continue; // e.g. a user record carrying only a tool_result
        }

        turns.push(Turn {
            uuid: rec.get("uuid").and_then(Value::as_str).unwrap_or("").to_string(),
            role,
            timestamp: rec
                .get("timestamp")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            blocks,
            model: msg.get("model").and_then(Value::as_str).map(str::to_string),
            usage: msg
                .get("usage")
                .and_then(|u| serde_json::from_value::<Usage>(u.clone()).ok()),
        });
    }

    // Calls still unresolved at the end of this batch stay pending for the next
    // poll. Ones carried in and still unresolved stay carried; ones opened in
    // this batch join them. The positions are only meaningful to the batch that
    // created them, which is why a later result becomes an update, not a mutation.
    for (id, pos) in pending {
        carried.insert(id, pos);
    }

    ParseOutput { turns, updates, pending: PendingCalls(carried) }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test conversation::parse::`
Expected: 10 tests pass.

- [ ] **Step 5: Verify against the largest real transcript**

Add this ignored test to the `tests` module:

```rust
    #[test]
    #[ignore]
    fn parses_the_largest_real_transcript() {
        use std::io::BufRead;
        let root = crate::index::projects_root();
        if !root.exists() {
            return;
        }
        // Find the largest depth-2 transcript.
        let mut biggest: Option<(u64, std::path::PathBuf)> = None;
        for e in walkdir::WalkDir::new(&root).max_depth(2).into_iter().filter_map(Result::ok) {
            if e.path().extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            let len = e.metadata().map(|m| m.len()).unwrap_or(0);
            if biggest.as_ref().map(|(b, _)| len > *b).unwrap_or(true) {
                biggest = Some((len, e.path().to_path_buf()));
            }
        }
        let Some((len, path)) = biggest else { return };
        let f = std::fs::File::open(&path).unwrap();
        let t0 = std::time::Instant::now();
        let turns = parse_records(
            std::io::BufReader::new(f).lines().map_while(Result::ok),
        )
        .turns;
        let elapsed = t0.elapsed();
        println!(
            "parsed {} bytes -> {} turns in {:?}",
            len,
            turns.len(),
            elapsed
        );
        assert!(!turns.is_empty(), "the largest transcript must yield turns");
        // Measured: 72 MB / 9,335 turns parses in ~151 ms release, ~1.27 s debug.
        // The shipped app is a release build, so that is the number the
        // criterion is about; the debug budget is deliberately loose so this
        // test is runnable during development without being a false alarm.
        let budget = if cfg!(debug_assertions) {
            std::time::Duration::from_secs(3)
        } else {
            std::time::Duration::from_secs(1)
        };
        assert!(
            elapsed < budget,
            "parse took {elapsed:?}, budget for this profile is {budget:?}"
        );
    }
```

Run: `cd src-tauri && cargo test conversation::parse::tests::parses_the_largest -- --ignored --nocapture`
Expected: prints byte count, turn count, and elapsed time. Report the printed numbers.

Then run it in release, which is the profile criterion 1 actually governs:

Run: `cd src-tauri && cargo test --release conversation::parse::tests::parses_the_largest -- --ignored --nocapture`
Expected: under 1 second. Report that number too — it is the one that counts.

- [ ] **Step 6: Verify lint and commit**

```bash
cd src-tauri && cargo clippy -- -D warnings
git add src-tauri/src/conversation
git commit -m "feat: parse transcript records into conversation turns"
```

---

### Task 3: Tail a transcript by byte offset

**Files:**
- Create: `src-tauri/src/conversation/tail.rs`
- Modify: `src-tauri/src/conversation/mod.rs` (add `pub mod tail;`)

**Interfaces:**
- Consumes: `conversation::parse::parse_records`, `conversation::model::{Turn, ConversationDelta}`
- Produces:
  - `conversation::tail::TailRead { turns: Vec<Turn>, updates: Vec<ToolResultUpdate>, offset: u64, reset: bool, pending: PendingCalls }`
  - `conversation::tail::read_from(path: &Path, offset: u64, pending: PendingCalls) -> std::io::Result<TailRead>`

A struct rather than a tuple: it carries five values, and a `(Vec, Vec, u64, bool, PendingCalls)`
tuple at every call site is unreadable. `reset` is true when the file shrank, meaning the caller
must discard prior turns. `pending` is threaded back in on the next call so a `tool_result` whose
`tool_use` arrived in an earlier poll is emitted as an update rather than dropped.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/conversation/tail.rs`:

```rust
use crate::conversation::model::{ToolResultUpdate, Turn};
use crate::conversation::parse::{parse_with_pending, PendingCalls};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

pub struct TailRead {
    pub turns: Vec<Turn>,
    pub updates: Vec<ToolResultUpdate>,
    pub offset: u64,
    pub reset: bool,
    pub pending: PendingCalls,
}

pub fn read_from(
    _path: &Path,
    _offset: u64,
    _pending: PendingCalls,
) -> std::io::Result<TailRead> {
    Ok(TailRead {
        turns: Vec::new(),
        updates: Vec::new(),
        offset: 0,
        reset: false,
        pending: PendingCalls::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const A1: &str = r#"{"type":"assistant","uuid":"a1","timestamp":"T1","message":{"role":"assistant","content":[{"type":"text","text":"first"}]}}"#;
    const A2: &str = r#"{"type":"assistant","uuid":"a2","timestamp":"T2","message":{"role":"assistant","content":[{"type":"text","text":"second"}]}}"#;

    fn write_file(dir: &std::path::Path, lines: &[&str]) -> std::path::PathBuf {
        let p = dir.join("t.jsonl");
        let mut f = std::fs::File::create(&p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        p
    }

    #[test]
    fn reads_everything_from_offset_zero() {
        let d = tempfile::tempdir().unwrap();
        let p = write_file(d.path(), &[A1, A2]);
        let r = read_from(&p, 0, PendingCalls::default()).unwrap();
        let (turns, offset, reset) = (r.turns, r.offset, r.reset);
        assert_eq!(turns.len(), 2);
        assert_eq!(offset, std::fs::metadata(&p).unwrap().len());
        assert!(!reset);
    }

    #[test]
    fn a_second_read_at_the_same_offset_returns_nothing() {
        let d = tempfile::tempdir().unwrap();
        let p = write_file(d.path(), &[A1, A2]);
        let first = read_from(&p, 0, PendingCalls::default()).unwrap();
        let r = read_from(&p, first.offset, first.pending).unwrap();
        let (turns, offset2, reset) = (r.turns, r.offset, r.reset);
        let offset = first.offset;
        assert!(turns.is_empty(), "an unchanged file must yield no new turns");
        assert_eq!(offset2, offset);
        assert!(!reset);
    }

    #[test]
    fn an_append_yields_only_the_new_turn() {
        let d = tempfile::tempdir().unwrap();
        let p = write_file(d.path(), &[A1]);
        let r0 = read_from(&p, 0, PendingCalls::default()).unwrap();
        let (first, offset) = (r0.turns, r0.offset);
        assert_eq!(first.len(), 1);

        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(f, "{A2}").unwrap();
        drop(f);

        let r1 = read_from(&p, offset, PendingCalls::default()).unwrap();
        let (new, reset) = (r1.turns, r1.reset);
        assert_eq!(new.len(), 1, "only the appended turn");
        assert_eq!(new[0].uuid, "a2");
        assert!(!reset);
    }

    #[test]
    fn a_truncated_file_signals_reset_and_re_reads() {
        let d = tempfile::tempdir().unwrap();
        let p = write_file(d.path(), &[A1, A2]);
        let offset = read_from(&p, 0, PendingCalls::default()).unwrap().offset;

        // Replace with a shorter file -- the offset is now past the end.
        write_file(d.path(), &[A1]);

        let r = read_from(&p, offset, PendingCalls::default()).unwrap();
        let (turns, new_offset, reset) = (r.turns, r.offset, r.reset);
        assert!(reset, "a shrunken file must signal reset");
        assert_eq!(turns.len(), 1, "and re-read from the start");
        assert_eq!(new_offset, std::fs::metadata(&p).unwrap().len());
    }

    #[test]
    fn a_missing_file_is_an_error_not_a_panic() {
        assert!(read_from(std::path::Path::new("/nonexistent/x.jsonl"), 0, PendingCalls::default()).is_err());
    }

    #[test]
    fn a_result_appended_after_its_call_surfaces_as_an_update() {
        // The realistic live case: a tool call is written, the poll fires, the
        // tool finishes seconds later. Measured: 60% of real calls take longer
        // than the 1s poll interval, so this is the common path, not an edge.
        const CALL: &str = r#"{"type":"assistant","uuid":"a1","timestamp":"T1","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"description":"Run tests"}}]}}"#;
        const RESULT: &str = r#"{"type":"user","uuid":"u1","timestamp":"T2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"all green"}]}}"#;

        let d = tempfile::tempdir().unwrap();
        let p = write_file(d.path(), &[CALL]);
        let first = read_from(&p, 0, PendingCalls::default()).unwrap();
        assert_eq!(first.turns.len(), 1);
        assert!(first.updates.is_empty());

        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(f, "{RESULT}").unwrap();
        drop(f);

        let second = read_from(&p, first.offset, first.pending).unwrap();
        assert!(second.turns.is_empty(), "no new turns, just a result");
        assert_eq!(second.updates.len(), 1, "the result must not be lost");
        assert_eq!(second.updates[0].result, "all green");
    }

    #[test]
    fn a_partial_trailing_line_is_not_consumed() {
        // A transcript being appended to right now can end mid-line. That
        // partial line must not be parsed, and the offset must stop before it
        // so the next poll picks it up whole.
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("t.jsonl");
        let mut f = std::fs::File::create(&p).unwrap();
        writeln!(f, "{A1}").unwrap();
        write!(f, "{{\"type\":\"assistant\",\"uuid\":\"partial\"").unwrap(); // no newline
        drop(f);

        let r = read_from(&p, 0, PendingCalls::default()).unwrap();
        let (turns, offset) = (r.turns, r.offset);
        assert_eq!(turns.len(), 1, "only the complete line parses");
        assert_eq!(
            offset,
            (A1.len() + 1) as u64,
            "offset must stop after the last complete line"
        );
    }
}
```

- [ ] **Step 2: Add `pub mod tail;` to `conversation/mod.rs` and run tests to verify they fail**

Run: `cd src-tauri && cargo test conversation::tail::`
Expected: 5 failures, 1 pass (`a_missing_file_is_an_error_not_a_panic` fails too — the stub returns Ok — so expect 6 failures).

- [ ] **Step 3: Implement**

Replace the `read_from` stub:

```rust
/// Read new complete lines from `offset`, returning parsed turns, the new
/// offset, and whether the caller must reset.
///
/// Only complete newline-terminated lines are consumed. A transcript being
/// appended to right now can end mid-line; parsing that would render a torn
/// record, and advancing past it would lose the line entirely.
pub fn read_from(
    path: &Path,
    offset: u64,
    pending: PendingCalls,
) -> std::io::Result<TailRead> {
    let len = std::fs::metadata(path)?.len();

    // A shorter file means truncation or replacement: start over, and drop the
    // carried pending state -- its indices refer to turns the client is about
    // to discard.
    let (start, reset) = if len < offset { (0, true) } else { (offset, false) };
    let carried = if reset { PendingCalls::default() } else { pending };

    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(start))?;

    let mut consumed = start;
    let mut complete: Vec<String> = Vec::new();
    let mut reader = BufReader::new(file);
    loop {
        let mut buf = String::new();
        let n = reader.read_line(&mut buf)?;
        if n == 0 {
            break;
        }
        if !buf.ends_with('\n') {
            break; // partial trailing line -- leave it for the next poll
        }
        consumed += n as u64;
        complete.push(buf);
    }

    let out = parse_with_pending(complete.into_iter(), carried);
    Ok(TailRead {
        turns: out.turns,
        updates: out.updates,
        offset: consumed,
        reset,
        pending: out.pending,
    })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test conversation::tail::`
Expected: 7 tests pass.

- [ ] **Step 5: Verify lint and commit**

```bash
cd src-tauri && cargo clippy -- -D warnings
git add src-tauri/src/conversation
git commit -m "feat: tail transcripts by byte offset"
```

---

### Task 4: Link subagent transcripts

**Files:**
- Create: `src-tauri/src/conversation/subagent.rs`
- Modify: `src-tauri/src/conversation/mod.rs` (add `pub mod subagent;`)

**Interfaces:**
- Consumes: nothing from earlier tasks
- Produces: `conversation::subagent::agent_id_from_result(result_text: &str) -> Option<String>` and `conversation::subagent::subagent_path(transcript: &Path, agent_id: &str) -> PathBuf`.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/conversation/subagent.rs`:

```rust
use std::path::{Path, PathBuf};

pub fn agent_id_from_result(_result_text: &str) -> Option<String> {
    None
}

pub fn subagent_path(_transcript: &Path, _agent_id: &str) -> PathBuf {
    PathBuf::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_agent_id_from_a_spawn_result() {
        // Real shape: the agentId is announced in the tool result text of the
        // Agent call that spawned the subagent.
        let text = "Async agent launched successfully. (This tool result is internal \
                    metadata.)\nagentId: a3643585f2c0a13f6 (internal ID - do not mention)";
        assert_eq!(
            agent_id_from_result(text).as_deref(),
            Some("a3643585f2c0a13f6")
        );
    }

    #[test]
    fn returns_none_when_no_agent_id_is_present() {
        assert!(agent_id_from_result("total 0\ndrwxr-xr-x  staff").is_none());
    }

    #[test]
    fn builds_the_subagent_path_beside_the_transcript() {
        // Real layout: <project-dir>/<session-uuid>/subagents/agent-<id>.jsonl
        let t = Path::new("/p/-Users-s-code-repo/5f34a680-16f5.jsonl");
        let got = subagent_path(t, "a07d46a9f4d4d19bd");
        assert_eq!(
            got,
            PathBuf::from("/p/-Users-s-code-repo/5f34a680-16f5/subagents/agent-a07d46a9f4d4d19bd.jsonl")
        );
    }

    #[test]
    fn a_transcript_with_no_parent_directory_degrades_gracefully() {
        let got = subagent_path(Path::new("bare.jsonl"), "a1");
        assert!(got.to_string_lossy().contains("agent-a1.jsonl"));
    }
}
```

- [ ] **Step 2: Add `pub mod subagent;` to `conversation/mod.rs` and run tests to verify they fail**

Run: `cd src-tauri && cargo test conversation::subagent::`
Expected: 3 failures, 1 pass (`returns_none_when_no_agent_id_is_present` passes vacuously against the `None` stub).

- [ ] **Step 3: Implement**

Replace both stubs:

```rust
/// Pull the `agentId: <id>` announcement out of an Agent tool result.
///
/// The id space differs from `tool_use_id` -- the only bridge between a
/// spawning call and its subagent transcript is this line in the result text.
pub fn agent_id_from_result(result_text: &str) -> Option<String> {
    let idx = result_text.find("agentId:")?;
    let rest = &result_text[idx + "agentId:".len()..];
    let id: String = rest
        .chars()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    if id.is_empty() {
        None
    } else {
        Some(id)
    }
}

/// `<dir>/<session-uuid>.jsonl` -> `<dir>/<session-uuid>/subagents/agent-<id>.jsonl`
pub fn subagent_path(transcript: &Path, agent_id: &str) -> PathBuf {
    let dir = transcript.parent().unwrap_or_else(|| Path::new(""));
    let stem = transcript
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    dir.join(stem)
        .join("subagents")
        .join(format!("agent-{agent_id}.jsonl"))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test conversation::subagent::`
Expected: 4 tests pass.

- [ ] **Step 5: Verify against the real tree**

Add this ignored test:

```rust
    #[test]
    #[ignore]
    fn links_real_subagent_files() {
        let root = crate::index::projects_root();
        if !root.exists() {
            return;
        }
        // Find a session directory that has a subagents/ child.
        let mut checked = 0;
        for e in walkdir::WalkDir::new(&root).max_depth(3).into_iter().filter_map(Result::ok) {
            if !e.path().ends_with("subagents") || !e.path().is_dir() {
                continue;
            }
            let session_dir = e.path().parent().unwrap();
            let transcript = session_dir.with_extension("jsonl");
            let files: Vec<_> = std::fs::read_dir(e.path())
                .unwrap()
                .filter_map(Result::ok)
                // Every transcript has a `.meta.json` sidecar beside it -- 1605
                // sidecars against 1598 transcripts across the tree. Without
                // this filter the id extractor is handed `agent-<id>.meta.json`
                // and produces `<id>.meta` as the "id".
                .filter(|f| {
                    f.path().extension().and_then(|x| x.to_str()) == Some("jsonl")
                })
                .collect();
            for f in files.iter().take(3) {
                let name = f.file_name().to_string_lossy().to_string();
                let id = name
                    .trim_start_matches("agent-")
                    .trim_end_matches(".jsonl")
                    .to_string();
                let built = subagent_path(&transcript, &id);
                assert!(built.exists(), "built path does not exist: {built:?}");
                checked += 1;
            }
            if checked >= 3 {
                break;
            }
        }
        println!("verified {checked} real subagent paths");
        assert!(checked > 0, "expected at least one real subagent file");
    }
```

Run: `cd src-tauri && cargo test conversation::subagent::tests::links_real -- --ignored --nocapture`
Expected: prints the number verified; passes.

- [ ] **Step 6: Verify lint and commit**

```bash
cd src-tauri && cargo clippy -- -D warnings
git add src-tauri/src/conversation
git commit -m "feat: link subagent transcripts to their spawning call"
```

---

### Task 5: Conversation Tauri commands

**Files:**
- Modify: `src-tauri/src/conversation/mod.rs` (add the commands)
- Modify: `src-tauri/src/lib.rs` (register them)

**Interfaces:**
- Consumes: everything from Tasks 1–4, plus `crate::index::projects_root` and `crate::transcript::parse_transcript`
- Produces three Tauri commands:
  - `load_conversation(sessionId: String) -> Result<Conversation, String>`
  - `poll_conversation(sessionId: String, offset: u64) -> Result<ConversationDelta, String>`
  - `load_subagent(sessionId: String, agentId: String) -> Result<Conversation, String>`

- [ ] **Step 1: Write the failing test for path resolution**

The commands need to find a session's transcript file from its id. Add to `src-tauri/src/conversation/mod.rs`:

```rust
pub mod model;
pub mod parse;
pub mod subagent;
pub mod tail;

use crate::index::projects_root;
use model::{Conversation, ConversationDelta};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// Locate a session's transcript by scanning project directories for
/// `<sessionId>.jsonl`. Depth 2 matches the index's own layout assumption.
pub fn transcript_path(root: &Path, session_id: &str) -> Option<PathBuf> {
    let target = format!("{session_id}.jsonl");
    walkdir::WalkDir::new(root)
        .max_depth(2)
        .into_iter()
        .filter_map(Result::ok)
        .find(|e| e.file_name().to_string_lossy() == target)
        .map(|e| e.path().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn finds_a_transcript_by_session_id() {
        let d = tempfile::tempdir().unwrap();
        let proj = d.path().join("-Users-s-code-repo");
        std::fs::create_dir_all(&proj).unwrap();
        let p = proj.join("abc-123.jsonl");
        let mut f = std::fs::File::create(&p).unwrap();
        writeln!(f, "{{}}").unwrap();
        drop(f);
        assert_eq!(transcript_path(d.path(), "abc-123"), Some(p));
    }

    #[test]
    fn returns_none_for_an_unknown_session() {
        let d = tempfile::tempdir().unwrap();
        assert!(transcript_path(d.path(), "nope").is_none());
    }

    #[test]
    fn does_not_match_a_subagent_file_at_depth_three() {
        // Subagent transcripts live deeper and must not be mistaken for the
        // session's own transcript.
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("proj").join("sess").join("subagents");
        std::fs::create_dir_all(&sub).unwrap();
        let mut f = std::fs::File::create(sub.join("agent-a1.jsonl")).unwrap();
        writeln!(f, "{{}}").unwrap();
        drop(f);
        assert!(transcript_path(d.path(), "agent-a1").is_none());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test conversation::tests::`
Expected: compile error or failures — `transcript_path` is defined but `walkdir` may need importing. Fix imports until it compiles, then expect all 3 to pass since the implementation is already written above. If they pass immediately, that is fine: this step is confirming the helper behaves, not TDD theatre.

- [ ] **Step 3: Add the three commands**

Append to `src-tauri/src/conversation/mod.rs`:

```rust
fn resolve(session_id: &str) -> Result<PathBuf, String> {
    transcript_path(&projects_root(), session_id)
        .ok_or_else(|| format!("no transcript found for session {session_id}"))
}

/// Pending tool calls per session, carried between polls.
///
/// Server-side because the positions are meaningless to the client -- it holds
/// rendered turns, not parse state. Keyed by session id; an entry is replaced
/// on every poll and dropped when a fresh `load_conversation` starts over.
static PENDING: LazyLock<Mutex<HashMap<String, parse::PendingCalls>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn take_pending(session_id: &str) -> parse::PendingCalls {
    PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(session_id)
        .unwrap_or_default()
}

fn store_pending(session_id: &str, pending: parse::PendingCalls) {
    PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(session_id.to_string(), pending);
}

#[tauri::command]
pub fn load_conversation(session_id: String) -> Result<Conversation, String> {
    let path = resolve(&session_id)?;
    // A fresh load starts over: discard any carried state for this session.
    let r = tail::read_from(&path, 0, parse::PendingCalls::default()).map_err(|e| {
        eprintln!("claudron: could not read conversation {session_id}: {e}");
        format!("could not read transcript: {e}")
    })?;
    store_pending(&session_id, r.pending);
    Ok(Conversation {
        session_id,
        turns: link_subagents(&path, r.turns),
        offset: r.offset,
    })
}

#[tauri::command]
pub fn poll_conversation(
    session_id: String,
    offset: u64,
) -> Result<ConversationDelta, String> {
    let path = resolve(&session_id)?;
    // Hold the lock across the whole read, not as a take/store pair. Two polls
    // for one session can overlap -- the UI polls on an interval -- and a
    // take-then-store would let the second start from an empty carry set and
    // silently drop pending calls, with the last store winning. Blocking
    // briefly is strictly better than losing results.
    //
    // On a read error the entry stays removed. That is deliberate: the next
    // successful poll rebuilds state, and keeping a carry set whose offsets may
    // no longer line up is worse than starting clean.
    let mut guard = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    let carried = guard.remove(&session_id).unwrap_or_default();
    let r = tail::read_from(&path, offset, carried).map_err(|e| {
        eprintln!("claudron: could not poll conversation {session_id}: {e}");
        format!("could not read transcript: {e}")
    })?;
    guard.insert(session_id.clone(), r.pending);
    drop(guard);
    Ok(ConversationDelta {
        turns: link_subagents(&path, r.turns),
        updates: r.updates,
        offset: r.offset,
        reset: r.reset,
    })
}

#[tauri::command]
pub fn load_subagent(
    session_id: String,
    agent_id: String,
) -> Result<Conversation, String> {
    let parent = resolve(&session_id)?;
    let path = subagent::subagent_path(&parent, &agent_id);
    // Subagent transcripts are loaded whole and never tailed, so they need no
    // carried state.
    let r = tail::read_from(&path, 0, parse::PendingCalls::default())
        .map_err(|_| "subagent transcript unavailable".to_string())?;
    Ok(Conversation { session_id, turns: r.turns, offset: r.offset })
}

/// Stamp `agent_id` onto any tool call whose result announced one, so the UI
/// knows which calls can expand into a nested conversation.
fn link_subagents(transcript: &Path, mut turns: Vec<model::Turn>) -> Vec<model::Turn> {
    for t in &mut turns {
        for b in &mut t.blocks {
            if let model::Block::Tool { call } = b {
                if let Some(res) = call.result.as_deref() {
                    if let Some(id) = subagent::agent_id_from_result(res) {
                        if subagent::subagent_path(transcript, &id).exists() {
                            call.agent_id = Some(id);
                        }
                    }
                }
            }
        }
    }
    turns
}
```

- [ ] **Step 4: Register the commands**

In `src-tauri/src/lib.rs`, extend the existing `invoke_handler` list:

```rust
        .invoke_handler(tauri::generate_handler![
            commands::list_sessions,
            commands::set_annotation,
            commands::focus_session,
            commands::resume_session,
            conversation::load_conversation,
            conversation::poll_conversation,
            conversation::load_subagent,
        ])
```

- [ ] **Step 5: Run the whole backend suite**

Run: `cd src-tauri && cargo test`
Expected: all tests pass, including Phase 1's. Then `cargo clippy -- -D warnings` exits 0 and `cargo build` succeeds.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src
git commit -m "feat: expose conversation loading and polling as Tauri commands"
```

---

### Task 6: Frontend conversation types and API

**Files:**
- Create: `src/types/conversation.ts`, `src/api/conversation.ts`, `src/api/conversation.test.ts`

**Interfaces:**
- Consumes: the three Tauri commands from Task 5
- Produces: TS types `Conversation`, `ConversationDelta`, `Turn`, `Block`, `ToolCall`, `Usage`, `Role`; and `loadConversation`, `pollConversation`, `loadSubagent`.

- [ ] **Step 1: Write the types**

Create `src/types/conversation.ts`:

```ts
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
```

- [ ] **Step 2: Write the failing API test**

Create `src/api/conversation.test.ts`:

```ts
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
```

- [ ] **Step 3: Run to verify it fails**

Run: `yarn vitest run src/api/conversation.test.ts`
Expected: FAIL — `./conversation` does not exist.

- [ ] **Step 4: Implement the API layer**

Create `src/api/conversation.ts`:

```ts
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
```

- [ ] **Step 5: Run to verify it passes, then commit**

Run: `yarn vitest run src/api/conversation.test.ts` (3 pass) and `yarn tsc --noEmit` (exit 0).

```bash
git add src/types src/api
git commit -m "feat: add typed frontend API for conversation loading"
```

---

### Task 7: Turn and tool-call components

**Files:**
- Create: `src/components/ToolCallBlock.tsx`, `src/components/TurnBlock.tsx`, `src/components/TurnBlock.test.tsx`

**Interfaces:**
- Consumes: `Turn`, `Block`, `ToolCall` from `src/types/conversation.ts`
- Produces: `TurnBlock({ turn, onExpandSubagent })` and `ToolCallBlock({ call, onExpandSubagent })`, where `onExpandSubagent: (agentId: string) => void`.

- [ ] **Step 1: Write the failing tests**

Create `src/components/TurnBlock.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { TurnBlock } from "./TurnBlock";
import type { Turn, ToolCall } from "../types/conversation";

const call = (over: Partial<ToolCall> = {}): ToolCall => ({
  id: "t1",
  name: "Bash",
  description: "List files",
  result: "total 0",
  isError: false,
  agentId: null,
  ...over,
});

const turn = (over: Partial<Turn> = {}): Turn => ({
  uuid: "u1",
  role: "assistant",
  timestamp: "2026-07-31T00:00:00Z",
  blocks: [{ kind: "text", text: "Hello there" }],
  model: "claude-opus-5",
  usage: null,
  ...over,
});

describe("TurnBlock", () => {
  it("renders prose text", () => {
    render(<TurnBlock turn={turn()} onExpandSubagent={vi.fn()} />);
    expect(screen.getByText("Hello there")).toBeDefined();
  });

  it("shows a tool call collapsed, using its description", () => {
    render(
      <TurnBlock turn={turn({ blocks: [{ kind: "tool", call: call() }] })} onExpandSubagent={vi.fn()} />,
    );
    expect(screen.getByText("List files")).toBeDefined();
    // Output is hidden until expanded.
    expect(screen.queryByText("total 0")).toBeNull();
  });

  it("falls back to the tool name when there is no description", () => {
    render(
      <TurnBlock
        turn={turn({ blocks: [{ kind: "tool", call: call({ description: null }) }] })}
        onExpandSubagent={vi.fn()}
      />,
    );
    expect(screen.getByText("Bash")).toBeDefined();
  });

  it("reveals the output when the tool call is clicked", () => {
    render(
      <TurnBlock turn={turn({ blocks: [{ kind: "tool", call: call() }] })} onExpandSubagent={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole("button", { name: /List files/ }));
    expect(screen.getByText("total 0")).toBeDefined();
  });

  it("marks an errored tool call", () => {
    render(
      <TurnBlock
        turn={turn({ blocks: [{ kind: "tool", call: call({ isError: true }) }] })}
        onExpandSubagent={vi.fn()}
      />,
    );
    expect(screen.getByText(/failed/i)).toBeDefined();
  });

  it("says so when a tool call produced no result", () => {
    render(
      <TurnBlock
        turn={turn({ blocks: [{ kind: "tool", call: call({ result: null }) }] })}
        onExpandSubagent={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /List files/ }));
    expect(screen.getByText(/no result/i)).toBeDefined();
  });

  it("offers to expand a subagent when one is linked", () => {
    const onExpand = vi.fn();
    render(
      <TurnBlock
        turn={turn({ blocks: [{ kind: "tool", call: call({ agentId: "a1" }) }] })}
        onExpandSubagent={onExpand}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /subagent/i }));
    expect(onExpand).toHaveBeenCalledWith("a1");
  });

  it("shows the model on an assistant turn", () => {
    render(<TurnBlock turn={turn()} onExpandSubagent={vi.fn()} />);
    expect(screen.getByText("claude-opus-5")).toBeDefined();
  });

  it("keeps a tool call expanded when its result arrives later", () => {
    // The measured common case: 60% of tool calls outlast the poll interval,
    // so the result patches into a turn the user may already have expanded.
    const before = turn({ blocks: [{ kind: "tool", call: call({ result: null }) }] });
    const { rerender } = render(<TurnBlock turn={before} onExpandSubagent={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: /List files/ }));
    expect(screen.getByText(/no result/i)).toBeDefined();

    // Same call id, now with a result -- as applyDelta would produce.
    const after = turn({ blocks: [{ kind: "tool", call: call({ result: "all green" }) }] });
    rerender(<TurnBlock turn={after} onExpandSubagent={vi.fn()} />);

    expect(screen.getByText("all green")).toBeDefined();
    expect(screen.queryByText(/no result/i)).toBeNull();
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `yarn vitest run src/components/TurnBlock.test.tsx`
Expected: FAIL — `./TurnBlock` does not exist.

- [ ] **Step 3: Implement ToolCallBlock**

Create `src/components/ToolCallBlock.tsx`:

```tsx
import { useState } from "react";
import type { ToolCall } from "../types/conversation";

/// Cap rendered output. A single `ls -la` result was already multi-KB in real
/// transcripts; sixty expanded would recreate the memory problem that made an
/// embedded terminal unattractive.
const MAX_RESULT_CHARS = 4000;

export function ToolCallBlock({
  call,
  onExpandSubagent,
}: {
  call: ToolCall;
  onExpandSubagent: (agentId: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [full, setFull] = useState(false);
  const label = call.description ?? call.name;
  const result = call.result;
  const truncated = result !== null && result.length > MAX_RESULT_CHARS && !full;
  const shown = truncated ? result.slice(0, MAX_RESULT_CHARS) : result;

  return (
    <div className="my-1">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-2 rounded px-2 py-1 text-left text-sm text-neutral-400 hover:bg-neutral-800/60"
      >
        <span className="shrink-0 text-neutral-600">{open ? "▾" : "▸"}</span>
        <span className="truncate">{label}</span>
        {call.isError && (
          <span className="shrink-0 rounded bg-red-500/15 px-1.5 text-[10px] text-red-400">
            failed
          </span>
        )}
      </button>

      {open && (
        <div className="mt-1 pl-6">
          {shown === null ? (
            <p className="text-xs italic text-neutral-500">No result — the session ended before this finished.</p>
          ) : (
            <pre className="overflow-x-auto whitespace-pre-wrap break-words rounded bg-neutral-900/80 p-2 text-xs text-neutral-300">
              {shown}
            </pre>
          )}
          {truncated && (
            <button
              type="button"
              onClick={() => setFull(true)}
              className="mt-1 text-[11px] text-sky-400 hover:underline"
            >
              Show full output ({result!.length.toLocaleString()} chars)
            </button>
          )}
        </div>
      )}

      {/* Outside the `open` block deliberately: a subagent link is about the
          call, not about its output, and burying it behind an expand step
          hides the nesting the conversation view exists to show. */}
      {call.agentId && (
        <button
          type="button"
          onClick={() => onExpandSubagent(call.agentId!)}
          className="ml-6 mt-1 block text-[11px] text-sky-400 hover:underline"
        >
          Show subagent conversation
        </button>
      )}
    </div>
  );
}
```

- [ ] **Step 4: Implement TurnBlock**

Create `src/components/TurnBlock.tsx`:

```tsx
import type { Turn } from "../types/conversation";
import { ToolCallBlock } from "./ToolCallBlock";

export function TurnBlock({
  turn,
  onExpandSubagent,
}: {
  turn: Turn;
  onExpandSubagent: (agentId: string) => void;
}) {
  const isUser = turn.role === "user";
  return (
    <article
      className={`px-4 py-2 ${isUser ? "border-l-2 border-l-sky-500/40 bg-neutral-800/30" : ""}`}
    >
      {turn.blocks.map((b, i) =>
        b.kind === "text" ? (
          <p
            key={`text-${i}`}
            className="whitespace-pre-wrap break-words text-sm leading-relaxed text-neutral-200"
          >
            {b.text}
          </p>
        ) : (
          // Key by the call's own id, not the array index. A late result
          // patches this turn in place -- an index key would tie identity to
          // position and silently collapse an expanded call if block order
          // ever changed.
          <ToolCallBlock key={b.call.id} call={b.call} onExpandSubagent={onExpandSubagent} />
        ),
      )}
      {turn.model && (
        <div className="mt-1 text-[10px] text-neutral-600">
          {turn.model}
          {turn.usage && ` · ${turn.usage.outputTokens.toLocaleString()} out`}
        </div>
      )}
    </article>
  );
}
```

- [ ] **Step 5: Run to verify tests pass, then commit**

Run: `yarn vitest run src/components/TurnBlock.test.tsx` (8 pass) and `yarn tsc --noEmit`.

```bash
git add src/components
git commit -m "feat: add turn and tool-call rendering components"
```

---

### Task 8: The conversation pane

**Files:**
- Create: `src/components/SubagentBlock.tsx`, `src/components/ConversationPane.tsx`, `src/components/ConversationPane.test.tsx`

**Interfaces:**
- Consumes: `loadConversation`, `pollConversation`, `loadSubagent`; `TurnBlock`
- Produces: `ConversationPane({ sessionId })`.

- [ ] **Step 1: Write the failing tests**

Create `src/components/ConversationPane.test.tsx`:

```tsx
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

  // The three below pin subagent behaviour. Without them, lazy loading, the
  // unavailable path, and session reset are verifiable only by reading the code.

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

  it("closes an expanded subagent when the selected session changes", async () => {
    // A discriminating reset test. Asserting only that the OLD turns vanished
    // would be a false positive: the ordinary `[data]` effect overwrites
    // `turns` on any new query result, so such a test passes even with no
    // sessionId reset at all. `openAgent` is cleared ONLY by that effect.
    loadConversation.mockResolvedValue({
      sessionId: "s1",
      turns: [withAgent("a1")],
      offset: 10,
    });
    loadSubagent.mockResolvedValue({
      sessionId: "s1",
      turns: [turn("s1", "nested content")],
      offset: 0,
    });

    const { rerender } = render(wrap(<ConversationPane sessionId="s1" />));
    await waitFor(() => expect(screen.getByText("Review the diff")).toBeDefined());

    fireEvent.click(screen.getByRole("button", { name: /subagent/i }));
    expect(await screen.findByText("nested content")).toBeDefined();

    // Switch sessions. The expanded subagent must not survive.
    loadConversation.mockResolvedValue({
      sessionId: "s2",
      turns: [turn("u2", "second session")],
      offset: 10,
    });
    rerender(wrap(<ConversationPane sessionId="s2" />));

    await waitFor(() => expect(screen.getByText("second session")).toBeDefined());
    expect(screen.queryByText("nested content")).toBeNull();
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
```

- [ ] **Step 2: Run to verify it fails**

Run: `yarn vitest run src/components/ConversationPane.test.tsx`
Expected: FAIL — `./ConversationPane` does not exist.

- [ ] **Step 3: Implement SubagentBlock**

Create `src/components/SubagentBlock.tsx`:

```tsx
import { useEffect, useState } from "react";
import { loadSubagent } from "../api/conversation";
import type { Turn } from "../types/conversation";
import { TurnBlock } from "./TurnBlock";

export function SubagentBlock({
  sessionId,
  agentId,
  onClose,
}: {
  sessionId: string;
  agentId: string;
  onClose: () => void;
}) {
  const [turns, setTurns] = useState<Turn[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    loadSubagent(sessionId, agentId)
      .then((c) => live && setTurns(c.turns))
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
    };
  }, [sessionId, agentId]);

  return (
    <section className="my-2 ml-6 border-l-2 border-l-purple-500/40 pl-3">
      <header className="flex items-center justify-between text-[11px] text-purple-300">
        <span>Subagent {agentId.slice(0, 8)}</span>
        <button type="button" onClick={onClose} className="hover:underline">
          hide
        </button>
      </header>
      {error && (
        <p role="alert" className="text-xs text-amber-400">
          Subagent transcript unavailable.
        </p>
      )}
      {!error && turns === null && <p className="text-xs text-neutral-500">Loading…</p>}
      {turns?.map((t) => (
        <TurnBlock key={t.uuid} turn={t} onExpandSubagent={() => {}} />
      ))}
    </section>
  );
}
```

Note the nested `onExpandSubagent` is a no-op: nesting stops at one level, deliberately. A subagent that spawns a subagent renders its tool call without an expand affordance.

- [ ] **Step 4: Implement ConversationPane**

Create `src/components/ConversationPane.tsx`:

```tsx
import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { loadConversation, pollConversation } from "../api/conversation";
import type { Turn, ToolResultUpdate } from "../types/conversation";
import { TurnBlock } from "./TurnBlock";
import { SubagentBlock } from "./SubagentBlock";

/// The open conversation polls far faster than the 10s session list: a tail
/// read is a few KB, and a slower cadence makes a live session feel dead.
const POLL_MS = 1000;

/// Append new turns and patch late-arriving results into turns already held.
///
/// Exported for testing. The patch half is load-bearing: measured, 60% of real
/// tool calls outlast the 1s poll interval, so their result arrives in a poll
/// after the one that delivered the call. Appending alone would leave most
/// calls reading "no result" forever.
export function applyDelta(
  prev: Turn[],
  incoming: Turn[],
  updates: ToolResultUpdate[],
): Turn[] {
  let next = updates.length === 0 ? prev : prev.map((t) => {
    const hit = t.blocks.some(
      (b) => b.kind === "tool" && updates.some((u) => u.toolUseId === b.call.id),
    );
    if (!hit) return t;
    return {
      ...t,
      blocks: t.blocks.map((b) => {
        if (b.kind !== "tool") return b;
        const u = updates.find((x) => x.toolUseId === b.call.id);
        if (!u) return b;
        return { ...b, call: { ...b.call, result: u.result, isError: u.isError } };
      }),
    };
  });
  return incoming.length ? [...next, ...incoming] : next;
}

export function ConversationPane({ sessionId }: { sessionId: string }) {
  const [turns, setTurns] = useState<Turn[]>([]);
  const [offset, setOffset] = useState<number | null>(null);
  const [openAgent, setOpenAgent] = useState<string | null>(null);
  const [stuck, setStuck] = useState(true);
  const scroller = useRef<HTMLDivElement | null>(null);

  const { data, error, isLoading } = useQuery({
    queryKey: ["conversation", sessionId],
    queryFn: () => loadConversation(sessionId),
  });

  // Reset all local state when the selected session changes.
  useEffect(() => {
    setTurns([]);
    setOffset(null);
    setOpenAgent(null);
    setStuck(true);
  }, [sessionId]);

  useEffect(() => {
    if (!data) return;
    setTurns(data.turns);
    setOffset(data.offset);
  }, [data]);

  // Tail the transcript.
  useEffect(() => {
    if (offset === null) return;
    let live = true;
    // Never let polls stack. `setInterval` fires on a timer regardless of
    // whether the previous call finished, and two overlapping polls for one
    // session contend for the backend's pending-call state. The backend holds
    // a lock so nothing is lost, but a stacked queue of slow polls is wasted
    // work either way.
    let inFlight = false;
    const id = setInterval(() => {
      if (inFlight) return;
      inFlight = true;
      void pollConversation(sessionId, offset)
        .then((d) => {
          if (!live) return;
          if (d.reset) {
            setTurns(d.turns);
          } else if (d.turns.length || d.updates.length) {
            setTurns((prev) => applyDelta(prev, d.turns, d.updates));
          }
          if (d.offset !== offset) setOffset(d.offset);
        })
        .catch(() => {
          /* transient read failure; the next tick retries */
        })
        // Last in the chain: clearing the flag earlier would let the next tick
        // start while this one's handler is still running.
        .finally(() => {
          inFlight = false;
        });
    }, POLL_MS);
    return () => {
      live = false;
      clearInterval(id);
    };
  }, [sessionId, offset]);

  // Stick to the bottom only while the user is already there.
  useEffect(() => {
    if (stuck && scroller.current) {
      scroller.current.scrollTop = scroller.current.scrollHeight;
    }
  }, [turns, stuck]);

  function onScroll() {
    const el = scroller.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    setStuck(atBottom);
  }

  if (error) {
    return (
      <div role="alert" className="p-4 text-sm text-amber-400">
        Could not load this conversation. {String(error)}
      </div>
    );
  }

  return (
    <div className="relative flex h-full flex-col">
      <div ref={scroller} onScroll={onScroll} className="min-h-0 flex-1 overflow-y-auto">
        {isLoading && <p className="p-4 text-sm text-neutral-500">Loading conversation…</p>}
        {!isLoading && turns.length === 0 && (
          <p className="p-4 text-sm text-neutral-500">Nothing to show for this session yet.</p>
        )}
        {turns.map((t) => (
          <div key={t.uuid}>
            <TurnBlock turn={t} onExpandSubagent={setOpenAgent} />
            {openAgent &&
              t.blocks.some((b) => b.kind === "tool" && b.call.agentId === openAgent) && (
                <SubagentBlock
                  sessionId={sessionId}
                  agentId={openAgent}
                  onClose={() => setOpenAgent(null)}
                />
              )}
          </div>
        ))}
      </div>
      {!stuck && (
        <button
          type="button"
          onClick={() => setStuck(true)}
          className="absolute bottom-3 right-4 rounded bg-sky-500/20 px-3 py-1 text-xs text-sky-300 hover:bg-sky-500/30"
        >
          Jump to latest
        </button>
      )}
    </div>
  );
}
```

- [ ] **Step 5: Run to verify tests pass, then commit**

Run: `yarn vitest run src/components/ConversationPane.test.tsx` (5 pass) and `yarn tsc --noEmit`.

```bash
git add src/components
git commit -m "feat: add the conversation pane with tailing and subagent expansion"
```

---

### Task 9: Wire the conversation into the app

**Files:**
- Create: `src/components/DetailSlideOver.tsx`
- Modify: `src/App.tsx`
- Modify: `src/App.test.tsx`

**Interfaces:**
- Consumes: `ConversationPane`, and Phase 1's `SessionDetail` unchanged
- Produces: the running app with the conversation as the right-hand pane.

- [ ] **Step 1: Write the failing tests**

Add to `src/App.test.tsx` (keep every existing test):

```tsx
  it("shows the conversation pane once a session is selected", async () => {
    listSessions.mockResolvedValue([mk()]);
    render(<App />);
    await waitFor(() => expect(screen.getByText("Fix the parser")).toBeDefined());
    fireEvent.click(screen.getByText("Fix the parser"));
    // The conversation pane owns the right side now.
    await waitFor(() => expect(screen.getByTestId("conversation-pane")).toBeDefined());
  });

  it("opens the details slide-over on demand", async () => {
    listSessions.mockResolvedValue([mk()]);
    render(<App />);
    await waitFor(() => expect(screen.getByText("Fix the parser")).toBeDefined());
    fireEvent.click(screen.getByText("Fix the parser"));
    expect(screen.queryByTestId("detail-slideover")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /details/i }));
    expect(screen.getByTestId("detail-slideover")).toBeDefined();
  });
```

`App.test.tsx` already mocks `../api/tauri`; add a mock for the conversation API too, near the existing one:

```tsx
vi.mock("./api/conversation", () => ({
  loadConversation: vi.fn().mockResolvedValue({ sessionId: "s1", turns: [], offset: 0 }),
  pollConversation: vi.fn().mockResolvedValue({ turns: [], updates: [], offset: 0, reset: false }),
  loadSubagent: vi.fn().mockResolvedValue({ sessionId: "s1", turns: [], offset: 0 }),
}));
```

- [ ] **Step 2: Run to verify the new tests fail**

Run: `yarn vitest run src/App.test.tsx`
Expected: the two new tests fail; the existing ones still pass.

- [ ] **Step 3: Implement the slide-over**

Create `src/components/DetailSlideOver.tsx`:

```tsx
import type { Annotation, Session } from "../types";
import { SessionDetail } from "./SessionDetail";

export function DetailSlideOver({
  session,
  onAnnotationChange,
  onClose,
}: {
  session: Session | null;
  onAnnotationChange: (a: Annotation) => void;
  onClose: () => void;
}) {
  return (
    <div className="absolute inset-0 z-10 flex">
      <button
        type="button"
        aria-label="Close details"
        onClick={onClose}
        className="flex-1 bg-black/40"
      />
      <aside
        data-testid="detail-slideover"
        className="w-96 overflow-y-auto border-l border-neutral-800 bg-neutral-900 shadow-xl"
      >
        <SessionDetail session={session} onAnnotationChange={onAnnotationChange} />
      </aside>
    </div>
  );
}
```

- [ ] **Step 4: Rewire App.tsx**

In `src/App.tsx`: import `ConversationPane` and `DetailSlideOver`, add `const [showDetails, setShowDetails] = useState(false);`, and replace the `<main>` element so the conversation is the pane and the detail moves into the slide-over.

Note the existing `<main>` is `className="flex min-w-0 flex-1 flex-col"`. Keep the flex
classes and add `relative` — the slide-over is absolutely positioned within it, so dropping
`flex-col` or `flex-1` collapses the layout:

```tsx
      <main className="relative flex min-w-0 flex-1 flex-col">
        {selected ? (
          <>
            <header className="flex shrink-0 items-center justify-between border-b border-neutral-800 px-4 py-2">
              <h2 className="truncate text-sm font-medium text-neutral-200">
                {shown?.annotation.displayName ?? selected.aiTitle ?? "Untitled session"}
              </h2>
              <button
                type="button"
                onClick={() => setShowDetails(true)}
                className="shrink-0 rounded bg-neutral-800 px-2 py-1 text-xs text-neutral-300 hover:bg-neutral-700"
              >
                Details &amp; notes
              </button>
            </header>
            {/* min-h-0 is required: without it a flex child refuses to shrink
                below its content and the pane's own scrolling never engages. */}
            <div data-testid="conversation-pane" className="min-h-0 flex-1">
              <ConversationPane sessionId={selected.sessionId} />
            </div>
          </>
        ) : (
          <div className="flex h-full items-center justify-center p-6 text-sm text-neutral-500">
            Select a session to see its conversation.
          </div>
        )}
        {showDetails && (
          <DetailSlideOver
            session={shown}
            onAnnotationChange={onAnnotationChange}
            onClose={() => setShowDetails(false)}
          />
        )}
      </main>
```

Also close the slide-over when the selected session changes — add `setShowDetails(false);` inside the existing `useEffect` that resets `draft` on `selectedId`.

- [ ] **Step 5: Run the whole suite**

Run: `yarn vitest run` — every test passes, including Phase 1's. Then `yarn tsc --noEmit` and `yarn build`.

- [ ] **Step 6: Commit**

```bash
git add src
git commit -m "feat: make the conversation the main pane, notes a slide-over"
```

---

### Task 10: Verify against real data

**Files:**
- Create: `docs/superpowers/plans/2026-07-31-phase-2a-verification.md`

**Interfaces:**
- Consumes: the complete application
- Produces: a written record of each success criterion, measured rather than asserted.

- [ ] **Step 1: Measure criterion 1 — largest transcript parses in under 1 second**

Run: `cd src-tauri && cargo test conversation::parse::tests::parses_the_largest -- --ignored --nocapture`
Record the printed byte count, turn count, and elapsed time.

- [ ] **Step 2: Measure criterion 5 — no record type is silently dropped**

Add this ignored test to `src-tauri/src/conversation/parse.rs`'s `tests` module and run it:

```rust
    #[test]
    #[ignore]
    fn every_real_record_type_is_handled_or_deliberately_ignored() {
        use std::collections::BTreeSet;
        use std::io::BufRead;
        // Every type observed in real transcripts. Conversation types become
        // turns; the rest are control-plane and are deliberately skipped.
        // agent-name / relocated / worktree-state are session metadata -- an
        // agent's display name, a moved cwd, worktree bookkeeping. None carries
        // a `message` field, so none is renderable conversation. They were
        // discovered during verification, when a capped scan missed them.
        const KNOWN: &[&str] = &[
            "assistant", "user", "attachment", "last-prompt", "ai-title",
            "queue-operation", "mode", "permission-mode", "pr-link", "system",
            "file-history-snapshot", "file-history-delta", "frame-link",
            "agent-name", "relocated", "worktree-state",
        ];
        let root = crate::index::projects_root();
        if !root.exists() {
            return;
        }
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut files = 0;
        for e in walkdir::WalkDir::new(&root).max_depth(2).into_iter().filter_map(Result::ok) {
            if e.path().extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            files += 1;
            let Ok(f) = std::fs::File::open(e.path()) else { continue };
            for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(t) = v.get("type").and_then(|x| x.as_str()) {
                        seen.insert(t.to_string());
                    }
                }
            }
        }
        println!("record types seen across {files} files: {seen:?}");
        let unknown: Vec<_> = seen.iter().filter(|t| !KNOWN.contains(&t.as_str())).collect();
        assert!(
            unknown.is_empty(),
            "unrecognised record type(s) {unknown:?} -- decide whether they render or are ignored, \
             then add them to KNOWN"
        );
    }
```

Run: `cd src-tauri && cargo test conversation::parse::tests::every_real_record -- --ignored --nocapture`
Record the printed list.

- [ ] **Step 3: Measure criteria 2, 3, and 4 in the running app**

Run `make dev`, select a session with a long history, and record:

- **Criterion 2** — warm poll under 50 ms. With the app running: `ps -eo pid,rss,comm | grep -i claudron` gives memory; for poll timing, watch that the UI does not stutter at the 1 s cadence, and note any visible lag.
- **Criterion 3** — a new turn appears within 2 s. Open a conversation for a session that is actively running, send it a message from its terminal, and time until the new turn appears in Claudron.
- **Criterion 4** — memory under 500 MB with the largest conversation open. Sum RSS across Claudron processes as above.

- [ ] **Step 4: Write the verification record**

Create `docs/superpowers/plans/2026-07-31-phase-2a-verification.md` with a table of the five criteria, the measured value for each, and PASS/FAIL. For any FAIL, record the observed value and the suspected cause.

- [ ] **Step 5: Commit**

```bash
git add docs
git commit -m "docs: record Phase 2A success criteria verification"
```

---

## Self-Review

**1. Spec coverage.** Every 2A requirement maps to a task:

| Spec requirement | Task |
|---|---|
| Prose, tool calls, errors, cost rendered | 1, 7 |
| `tool_result.content` string *and* list forms | 2 |
| User content string *and* list forms | 2 |
| Orphaned tool call renders | 2, 7 |
| Control-plane records ignored | 2 |
| Malformed record never blanks the view | 2 |
| Ordering and timing | 1, 2 |
| Subagent linking via `agentId` | 4, 5 |
| Lazy subagent loading | 5, 8 |
| Subagent unavailable degrades | 8 |
| Tail by byte offset | 3 |
| Truncation / inode change → reset | 3, 8 |
| Partial trailing line not consumed | 3 |
| Two polling rates (1 s conversation) | 8 |
| Auto-scroll sticks and releases | 8 |
| Enormous result truncated | 7 |
| Conversation is the right pane | 9 |
| `SessionDetail` preserved intact in a slide-over | 9 |
| Success criteria measured | 10 |

2B requirements (tmux, reply box, spawning) are deliberately absent — they are a separate plan.

**2. Placeholder scan.** No TBDs, no "add error handling", no "similar to Task N". Every code step contains complete code.

**3. Type consistency.** `Turn`, `Block`, `ToolCall`, `Usage`, `ToolResultUpdate`, `Conversation`, `ConversationDelta` match between `conversation/model.rs` (camelCase via serde) and `src/types/conversation.ts`. Command names match between `conversation/mod.rs` (`load_conversation`, `poll_conversation`, `load_subagent`) and `src/api/conversation.ts`. `onExpandSubagent: (agentId: string) => void` is consistent across `TurnBlock`, `ToolCallBlock`, and `ConversationPane`. `read_from(path, offset, pending) -> TailRead` in Task 3 is consumed field-wise in Task 5. `parse_with_pending` returns `ParseOutput { turns, updates, pending }` in Task 2 and is consumed that way in Task 3.

**4. The tool-result patch channel.** The single most important cross-task
invariant, added after a review found the original design silently dropped most
tool results on live sessions. It spans four tasks and must stay consistent:
Task 1 defines `ToolResultUpdate` and `ConversationDelta.updates`; Task 2 emits
updates for calls it did not open and returns carry-forward `pending`; Task 3
threads `pending` through `read_from`, dropping it on `reset`; Task 5 stores
`pending` per session between polls; Task 8's `applyDelta` patches held turns.
Break any link and results vanish silently — measured at 60% of real calls.
