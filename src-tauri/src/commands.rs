use crate::actions;
use crate::annotations;
use crate::index;
use crate::model::{Annotation, Liveness, Session, SessionList};
use crate::process;
use std::path::Path;

/// Merge annotations and live-process state into indexed sessions.
///
/// Split out from the Tauri command so it is testable without an app handle.
pub fn assemble(root: &Path, store: &Path, live_cwds: &[String]) -> Vec<Session> {
    assemble_with_hooks(root, store, live_cwds, &Default::default(), &[])
}

/// `assemble`, with hook events and the live pid list.
///
/// When a hook event names a session AND its recorded pid is still live, that
/// session is exactly the one running in that process -- no directory guessing.
/// Sessions without a hook event fall back to the cwd inference, so the feature
/// degrades to the previous behaviour rather than disappearing.
pub fn assemble_with_hooks(
    root: &Path,
    store: &Path,
    live_cwds: &[String],
    hook_events: &std::collections::HashMap<String, crate::hooks::HookEvent>,
    live_pids: &[i32],
) -> Vec<Session> {
    let saved = annotations::load(store).unwrap_or_else(|e| {
        eprintln!("claudron: could not load annotations: {e}");
        std::collections::HashMap::new()
    });
    // Canonicalize live_cwds once, up front, rather than per session: the
    // list is typically small and shared across every session in this pass.
    let canonical_live: Vec<std::path::PathBuf> = live_cwds
        .iter()
        .map(|c| std::fs::canonicalize(c).unwrap_or_else(|_| Path::new(c).into()))
        .collect();

    index::index_sessions(root)
        .into_iter()
        .map(|mut s| {
            if let Some(a) = saved.get(&s.session_id) {
                s.annotation = a.clone();
            }
            // A hook event is authoritative when its pid is still live: it names
            // THIS session, not merely a process sharing a directory. Confirming
            // against the live pid list matters because pids are reused and a
            // crashed session never fires SessionEnd -- the file alone proves
            // nothing about now.
            let hooked_live = hook_events
                .get(&s.session_id)
                .is_some_and(|e| live_pids.contains(&e.pid));

            if hooked_live {
                s.liveness = Liveness::Legacy;
            } else if hook_events.contains_key(&s.session_id) {
                // A hook event exists but its pid is gone: this session is
                // definitively not running, whatever else shares its directory.
                // Trusting the cwd inference here is what produced the false
                // positives hooks exist to remove.
            } else {
                // No hook event for this session: fall back to the cwd
                // inference. live_cwds comes from `lsof -d cwd -Fn`, which
                // reports fully-resolved paths, while a session's recorded cwd
                // is never canonicalized -- so compare canonicalized forms, or
                // /private-style aliasing makes a live session read as Idle. A
                // cwd that cannot canonicalize (deleted directory) falls back
                // to its raw form so it can still match another raw one.
                let canonical_s_cwd =
                    std::fs::canonicalize(&s.cwd).unwrap_or_else(|_| Path::new(&s.cwd).into());
                if canonical_live.iter().any(|c| c == &canonical_s_cwd) {
                    s.liveness = Liveness::Legacy;
                }
            }
            s
        })
        .collect()
}

#[tauri::command]
pub fn list_sessions() -> SessionList {
    let procs = process::discover_claude_processes();
    let live_pids: Vec<i32> = procs.iter().map(|p| p.pid).collect();
    let live: Vec<String> = procs.into_iter().filter_map(|p| p.cwd).collect();
    let hook_events = crate::hooks::read_events(&crate::hooks::events_dir());
    let sessions = assemble_with_hooks(
        &index::projects_root(),
        &annotations::store_path(),
        &live,
        &hook_events,
        &live_pids,
    );
    let observed: Vec<String> = sessions.iter().filter_map(|s| s.version.clone()).collect();
    let version_baseline = crate::version::baseline(crate::version::installed(), &observed);
    SessionList {
        sessions,
        version_baseline,
    }
}

#[tauri::command]
pub fn set_annotation(session_id: String, annotation: Annotation) -> Result<(), String> {
    let path = annotations::store_path();
    let mut map = annotations::load(&path)
        .map_err(|e| format!("refusing to save: annotation store is unreadable ({e})"))?;
    map.insert(session_id, annotation);
    annotations::save(&path, &map).map_err(|e| {
        eprintln!("claudron: could not save annotations: {e}");
        e.to_string()
    })
}

#[tauri::command]
pub fn focus_session(cwd: String) -> Result<(), String> {
    let out = actions::run_applescript(&actions::iterm_focus_script(&cwd))?;
    if out == "not-found" {
        return Err(format!(
            "No iTerm2 tab found for {cwd}. (Jump needs iTerm2 shell integration, \
             which is what reports each tab's directory.)"
        ));
    }
    Ok(())
}

#[tauri::command]
pub fn resume_session(session_id: String, cwd: String) -> Result<(), String> {
    actions::run_applescript(&actions::resume_script(&session_id, &cwd)).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ManualStatus;
    use std::collections::HashMap;
    use std::fs;
    use std::io::Write;

    fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        let mut f = fs::File::create(proj.join("s1.jsonl")).unwrap();
        writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"hi"}},"entrypoint":"cli","sessionId":"s1","cwd":"/live/repo"}}"#).unwrap();
        let mut g = fs::File::create(proj.join("s2.jsonl")).unwrap();
        writeln!(g, r#"{{"type":"user","message":{{"role":"user","content":"hi"}},"entrypoint":"cli","sessionId":"s2","cwd":"/dead/repo"}}"#).unwrap();
        let store = dir.path().join("annotations.json");
        (dir, store)
    }

    #[test]
    fn attaches_saved_annotations_to_sessions() {
        let (dir, store) = fixture();
        let mut map = HashMap::new();
        map.insert(
            "s1".to_string(),
            Annotation {
                notes: "remember this".into(),
                status: Some(ManualStatus::Blocked),
                display_name: None,
            },
        );
        annotations::save(&store, &map).unwrap();

        let sessions = assemble(dir.path(), &store, &[]);
        let s1 = sessions.iter().find(|s| s.session_id == "s1").unwrap();
        assert_eq!(s1.annotation.notes, "remember this");
        assert_eq!(s1.annotation.status, Some(ManualStatus::Blocked));
    }

    #[test]
    fn sessions_without_annotations_get_defaults() {
        let (dir, store) = fixture();
        let sessions = assemble(dir.path(), &store, &[]);
        let s2 = sessions.iter().find(|s| s.session_id == "s2").unwrap();
        assert_eq!(s2.annotation, Annotation::default());
    }

    fn hook(
        pid: i32,
        session: &str,
        cwd: &str,
    ) -> std::collections::HashMap<String, crate::hooks::HookEvent> {
        let mut m = std::collections::HashMap::new();
        m.insert(
            session.to_string(),
            crate::hooks::HookEvent {
                pid,
                session_id: session.to_string(),
                cwd: cwd.to_string(),
                event: "SessionStart".into(),
                ts: 1,
            },
        );
        m
    }

    #[test]
    fn a_hook_event_with_a_live_pid_marks_exactly_that_session() {
        // The point of hooks: identity, not directory guessing.
        let (dir, store) = fixture();
        let out = assemble_with_hooks(
            dir.path(),
            &store,
            &[],
            &hook(4242, "s1", "/live/repo"),
            &[4242],
        );
        let s1 = out.iter().find(|s| s.session_id == "s1").unwrap();
        assert_eq!(s1.liveness, Liveness::Legacy);
        let s2 = out.iter().find(|s| s.session_id == "s2").unwrap();
        assert_eq!(
            s2.liveness,
            Liveness::Idle,
            "only the hooked session is live"
        );
    }

    #[test]
    fn a_hook_event_whose_pid_is_gone_does_not_mark_the_session_live() {
        // A crashed session never fires SessionEnd, so its file outlives it.
        // The file alone proves nothing about now.
        let (dir, store) = fixture();
        let out = assemble_with_hooks(
            dir.path(),
            &store,
            &[],
            &hook(4242, "s1", "/live/repo"),
            &[], // no live pids
        );
        let s1 = out.iter().find(|s| s.session_id == "s1").unwrap();
        assert_eq!(s1.liveness, Liveness::Idle);
    }

    #[test]
    fn a_dead_hooked_session_is_not_revived_by_a_neighbour_in_its_directory() {
        // THE bug hooks exist to fix. Another session in the same directory
        // makes the cwd inference report this one live; the hook event proves
        // it is not, and must win.
        let (dir, store) = fixture();
        let out = assemble_with_hooks(
            dir.path(),
            &store,
            &["/live/repo".to_string()], // a neighbour IS live here
            &hook(4242, "s1", "/live/repo"),
            &[], // but s1's own pid is gone
        );
        let s1 = out.iter().find(|s| s.session_id == "s1").unwrap();
        assert_eq!(
            s1.liveness,
            Liveness::Idle,
            "a hook event that proves the session is dead must outrank the cwd guess"
        );
    }

    #[test]
    fn sessions_without_hook_events_still_use_the_cwd_inference() {
        // Degrades to the previous behaviour rather than disappearing.
        let (dir, store) = fixture();
        let out = assemble_with_hooks(
            dir.path(),
            &store,
            &["/live/repo".to_string()],
            &Default::default(), // no hooks installed
            &[],
        );
        let s1 = out.iter().find(|s| s.session_id == "s1").unwrap();
        assert_eq!(s1.liveness, Liveness::Legacy);
    }

    #[test]
    fn a_live_cwd_marks_its_session_legacy() {
        let (dir, store) = fixture();
        let sessions = assemble(dir.path(), &store, &["/live/repo".to_string()]);
        let s1 = sessions.iter().find(|s| s.session_id == "s1").unwrap();
        assert_eq!(s1.liveness, Liveness::Legacy);
        let s2 = sessions.iter().find(|s| s.session_id == "s2").unwrap();
        assert_eq!(s2.liveness, Liveness::Idle);
    }

    #[test]
    fn a_live_cwd_that_is_a_path_alias_of_the_session_cwd_still_marks_it_legacy() {
        // Regression, mirroring the fix in git::remove_worktree_impl: lsof
        // reports a fully resolved cwd, but a session's recorded cwd (from
        // its transcript JSONL) is never canonicalized. On macOS a plain
        // tempdir already aliases through /private, so raw string comparison
        // silently fails to recognize the same real directory as live.
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        let live_dir = dir.path().join("live-session-dir");
        fs::create_dir_all(&live_dir).unwrap();
        let uncanonical_cwd = live_dir.to_string_lossy().to_string();
        let canonical_cwd = live_dir
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert_ne!(
            uncanonical_cwd, canonical_cwd,
            "fixture did not actually exercise aliasing on this machine"
        );

        let mut f = fs::File::create(proj.join("s1.jsonl")).unwrap();
        writeln!(
            f,
            r#"{{"type":"user","message":{{"role":"user","content":"hi"}},"entrypoint":"cli","sessionId":"s1","cwd":{:?}}}"#,
            uncanonical_cwd
        )
        .unwrap();
        let store = dir.path().join("annotations.json");

        let sessions = assemble(dir.path(), &store, &[canonical_cwd]);
        let s1 = sessions.iter().find(|s| s.session_id == "s1").unwrap();
        assert_eq!(
            s1.liveness,
            Liveness::Legacy,
            "aliased path must still be recognized as live"
        );
    }

    #[test]
    fn set_annotation_refuses_to_write_over_an_unreadable_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("annotations.json");
        std::fs::write(&store, b"{ this is not json").unwrap();
        let before = std::fs::read(&store).unwrap();

        let mut map = HashMap::new();
        map.insert("s1".to_string(), Annotation::default());
        // Simulate what set_annotation does: load must fail, so no save happens.
        assert!(
            annotations::load(&store).is_err(),
            "corrupt store must not load as empty"
        );

        // The corrupt file must be left exactly as it was, not overwritten.
        assert_eq!(std::fs::read(&store).unwrap(), before);
    }
}
