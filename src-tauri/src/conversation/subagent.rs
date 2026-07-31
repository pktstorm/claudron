use std::path::{Path, PathBuf};

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
                // Every transcript has a `.meta.json` sidecar beside it (1605
                // sidecars vs 1598 transcripts across the tree). Only the
                // .jsonl files are transcripts.
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
}
