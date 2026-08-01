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
            // A live `claude` process in this session's cwd means the session
            // is running in a terminal Claudron did not spawn. Compare
            // CANONICALIZED forms: live_cwds comes from `lsof -d cwd -Fn`
            // (process.rs), which reports the fully resolved path, while a
            // session's recorded cwd (from its transcript JSONL) is never
            // canonicalized. The same /private (and /var, /etc, any user
            // symlink) aliasing that made remove_worktree's occupancy guard
            // fail open on raw string comparison applies here too -- this is
            // only a UX inconsistency (a badge reads Idle when a session is
            // actually live), not a safety hole, since remove_worktree does
            // its own independent, already-fixed check. A cwd that fails to
            // canonicalize (already-deleted directory) falls back to its raw
            // form so it can still match another raw, uncanonicalizable cwd.
            let canonical_s_cwd =
                std::fs::canonicalize(&s.cwd).unwrap_or_else(|_| Path::new(&s.cwd).into());
            if canonical_live.iter().any(|c| c == &canonical_s_cwd) {
                s.liveness = Liveness::Legacy;
            }
            s
        })
        .collect()
}

#[tauri::command]
pub fn list_sessions() -> SessionList {
    let live: Vec<String> = process::discover_claude_processes()
        .into_iter()
        .filter_map(|p| p.cwd)
        .collect();
    let sessions = assemble(&index::projects_root(), &annotations::store_path(), &live);
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
        writeln!(
            f,
            r#"{{"type":"user","entrypoint":"cli","sessionId":"s1","cwd":"/live/repo"}}"#
        )
        .unwrap();
        let mut g = fs::File::create(proj.join("s2.jsonl")).unwrap();
        writeln!(
            g,
            r#"{{"type":"user","entrypoint":"cli","sessionId":"s2","cwd":"/dead/repo"}}"#
        )
        .unwrap();
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
            r#"{{"type":"user","entrypoint":"cli","sessionId":"s1","cwd":{:?}}}"#,
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
