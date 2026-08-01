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

#[tauri::command]
pub fn load_conversation(session_id: String) -> Result<Conversation, String> {
    let path = resolve(&session_id)?;
    // Hold the lock across the whole read, matching poll_conversation -- see
    // the comment there for why take-then-store is not safe under overlap.
    // A fresh load starts over regardless: discard any carried state for this
    // session by reading with a default PendingCalls.
    let mut guard = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    let r = tail::read_from(&path, 0, parse::PendingCalls::default()).map_err(|e| {
        eprintln!("claudron: could not read conversation {session_id}: {e}");
        format!("could not read transcript: {e}")
    })?;
    // Note: the `?` above returns early on a read error, leaving no entry
    // stored for this session -- acceptable, since the next successful call
    // rebuilds state from scratch anyway.
    guard.insert(session_id.clone(), r.pending);
    drop(guard);
    Ok(Conversation {
        session_id,
        turns: link_subagents(&path, r.turns),
        offset: r.offset,
    })
}

#[tauri::command]
pub fn poll_conversation(session_id: String, offset: u64) -> Result<ConversationDelta, String> {
    let path = resolve(&session_id)?;
    // Hold the lock across the whole read. Two polls for the same session can
    // overlap (the UI polls on an interval), and a take-then-store pair would
    // let the second start from an empty carry set and silently drop pending
    // calls. Blocking briefly is strictly better than losing results.
    let mut guard = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    let carried = guard.remove(&session_id).unwrap_or_default();
    let r = tail::read_from(&path, offset, carried).map_err(|e| {
        eprintln!("claudron: could not poll conversation {session_id}: {e}");
        format!("could not read transcript: {e}")
    })?;
    // Note: the `?` above returns early on a read error, leaving the entry
    // removed rather than restored -- acceptable, since the next successful
    // poll rebuilds state from a fresh read at the caller's last-known offset.
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
pub fn load_subagent(session_id: String, agent_id: String) -> Result<Conversation, String> {
    let parent = resolve(&session_id)?;
    let path = subagent::subagent_path(&parent, &agent_id);
    // Subagent transcripts are loaded whole and never tailed, so they need no
    // carried state.
    let r = tail::read_from(&path, 0, parse::PendingCalls::default())
        .map_err(|_| "subagent transcript unavailable".to_string())?;
    Ok(Conversation {
        session_id,
        turns: r.turns,
        offset: r.offset,
    })
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

    /// CLAUDRON_PROJECTS_DIR is process-global; serialize tests that set it.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::LazyLock<std::sync::Mutex<()>> =
            std::sync::LazyLock::new(|| std::sync::Mutex::new(()));
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn a_late_result_survives_a_poll_boundary_through_the_command_layer() {
        // The measured common case: 60% of tool calls outlast the 1s poll, so
        // the result arrives in a later poll than the call. This exercises the
        // PENDING map itself -- the parse/tail tests never reach it.
        const CALL: &str = r#"{"type":"assistant","uuid":"a1","timestamp":"T1","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"description":"Run tests"}}]}}"#;
        const RESULT: &str = r#"{"type":"user","uuid":"u1","timestamp":"T2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"all green"}]}}"#;

        let _guard = env_lock();
        let d = tempfile::tempdir().unwrap();
        let proj = d.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        let p = proj.join("sess-late.jsonl");
        std::fs::write(&p, format!("{CALL}\n")).unwrap();

        std::env::set_var("CLAUDRON_PROJECTS_DIR", d.path());

        let first = load_conversation("sess-late".to_string()).unwrap();
        assert_eq!(first.turns.len(), 1);

        // The tool finishes after the first read -- a later poll sees the result.
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(f, "{RESULT}").unwrap();
        drop(f);

        let delta = poll_conversation("sess-late".to_string(), first.offset).unwrap();
        assert!(delta.turns.is_empty(), "no new turns, just a result");
        assert_eq!(
            delta.updates.len(),
            1,
            "the late result must survive via PENDING"
        );
        assert_eq!(delta.updates[0].tool_use_id, "t1");
        assert_eq!(delta.updates[0].result, "all green");

        std::env::remove_var("CLAUDRON_PROJECTS_DIR");
    }
}
