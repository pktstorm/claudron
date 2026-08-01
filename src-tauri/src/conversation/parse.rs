use crate::conversation::model::{Block, Role, ToolCall, ToolResultUpdate, Turn, Usage};
use serde_json::Value;
use std::collections::HashMap;

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
        let Some(msg) = rec.get("message") else {
            continue;
        };

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
                                    blocks.push(Block::Text {
                                        text: t.to_string(),
                                    });
                                }
                            }
                        }
                        Some("tool_use") => {
                            let id = b
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
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
            uuid: rec
                .get("uuid")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
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

    ParseOutput {
        turns,
        updates,
        pending: PendingCalls(carried),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> std::vec::IntoIter<String> {
        v.iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
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
        assert_eq!(
            turns[0].blocks,
            vec![Block::Text {
                text: "Hello there".into()
            }]
        );
    }

    #[test]
    fn parses_a_user_turn_whose_content_is_a_bare_string() {
        // Measured: user message.content is a bare string 1148 times.
        let turns = parse_records(lines(&[
            r#"{"type":"user","uuid":"u2","timestamp":"T2","message":{"role":"user","content":"just text"}}"#,
        ])).turns;
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].role, Role::User);
        assert_eq!(
            turns[0].blocks,
            vec![Block::Text {
                text: "just text".into()
            }]
        );
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
        assert!(
            second.turns.is_empty(),
            "a result-only batch yields no new turns"
        );
        assert_eq!(
            second.updates.len(),
            1,
            "the result must survive as an update"
        );
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
        assert_eq!(
            turns.len(),
            1,
            "the tool_result-only user turn must not render"
        );
    }

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
        for e in walkdir::WalkDir::new(&root)
            .max_depth(2)
            .into_iter()
            .filter_map(Result::ok)
        {
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
        let turns = parse_records(std::io::BufReader::new(f).lines().map_while(Result::ok)).turns;
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

    #[test]
    #[ignore]
    fn every_real_record_type_is_handled_or_deliberately_ignored() {
        use std::collections::BTreeSet;
        use std::io::BufRead;
        // Every type observed in real transcripts. Conversation types become
        // turns; the rest are control-plane and are deliberately skipped.
        //
        // agent-name / relocated / worktree-state are session metadata -- an
        // agent's display name, a moved cwd, worktree bookkeeping. None carries
        // a `message` field, so none is renderable conversation.
        const KNOWN: &[&str] = &[
            "assistant",
            "user",
            "attachment",
            "last-prompt",
            "ai-title",
            "queue-operation",
            "mode",
            "permission-mode",
            "pr-link",
            "system",
            "file-history-snapshot",
            "file-history-delta",
            "frame-link",
            "agent-name",
            "relocated",
            "worktree-state",
        ];
        let root = crate::index::projects_root();
        if !root.exists() {
            return;
        }
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut files = 0;
        for e in walkdir::WalkDir::new(&root)
            .max_depth(2)
            .into_iter()
            .filter_map(Result::ok)
        {
            if e.path().extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            files += 1;
            let Ok(f) = std::fs::File::open(e.path()) else {
                continue;
            };
            for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(t) = v.get("type").and_then(|x| x.as_str()) {
                        seen.insert(t.to_string());
                    }
                }
            }
        }
        println!("record types seen across {files} files: {seen:?}");
        let unknown: Vec<_> = seen
            .iter()
            .filter(|t| !KNOWN.contains(&t.as_str()))
            .collect();
        assert!(
            unknown.is_empty(),
            "unrecognised record type(s) {unknown:?} -- decide whether they render or are ignored, \
             then add them to KNOWN"
        );
    }
}
