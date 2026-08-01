use crate::model::{Annotation, Liveness, Session};
use crate::project::project_label;
use crate::transcript::parse_transcript;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use walkdir::WalkDir;

/// Freshness key for a cached parse: full-precision mtime plus file size.
///
/// Whole-second mtime is NOT sufficient -- an active session writes its
/// transcript several times per second, so a second-granularity key serves a
/// stale parse for exactly the live sessions the dashboard exists to show.
/// Size is a cheap second signal for the rare same-nanosecond case.
type Freshness = (u128, u64);

/// Cache of parsed sessions keyed by transcript path, with the freshness key
/// the parse was made from. A poll re-parses a file only when that key moved.
///
/// The cached value is `Option<Session>` rather than `Session`: most transcripts
/// on a real tree are *rejected* by `parse_transcript` (sdk sessions, sidechains,
/// no entrypoint) and would otherwise never get an entry, so every poll would
/// re-parse them forever. Caching the rejection (`None`) too means a poll only
/// ever re-parses files whose freshness key actually moved.
///
/// Without this, every poll re-parses all 1312 transcripts (~10s measured), which
/// is longer than any sane poll interval.
///
/// `HashMap::new()` is not a const fn, so a plain `Mutex::new(HashMap::new())`
/// cannot be a `static` initializer here; `LazyLock` (stable std, no new crate)
/// defers construction to first access instead.
type CacheEntry = (Freshness, Option<Session>);
static CACHE: LazyLock<Mutex<HashMap<PathBuf, CacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn projects_root() -> PathBuf {
    if let Ok(p) = std::env::var("CLAUDRON_PROJECTS_DIR") {
        return PathBuf::from(p);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join(".claude")
        .join("projects")
}

/// Rank for sidebar ordering. Lower sorts first.
///
/// `Legacy` and `Managed` tie: both mean a live process, and how the session is
/// hosted is not a reason to rank one above the other.
fn liveness_rank(l: Liveness) -> u8 {
    match l {
        Liveness::Legacy | Liveness::Managed => 0,
        Liveness::Interrupted => 1,
        Liveness::Idle => 2,
    }
}

/// Live sessions first, then most-recently-active within each rank.
///
/// Extracted from `index_sessions` so the ordering can be tested without
/// building a transcript tree.
fn sort_sessions(sessions: &mut [Session]) {
    sessions.sort_by(|a, b| {
        liveness_rank(a.liveness)
            .cmp(&liveness_rank(b.liveness))
            .then(b.last_activity.cmp(&a.last_activity))
            // Final tiebreak: last_activity is whole seconds, so co-active
            // sessions collide. Without this the order falls to WalkDir
            // traversal, and rows jump between polls.
            .then(a.session_id.cmp(&b.session_id))
    });
}

pub fn index_sessions(root: &Path) -> Vec<Session> {
    let mut out = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());

    for entry in WalkDir::new(root)
        .max_depth(2)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }

        let meta = entry.metadata().ok();
        let modified = meta.as_ref().and_then(|m| m.modified().ok());
        let freshness: Freshness = (
            modified
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            meta.as_ref().map(|m| m.len()).unwrap_or(0),
        );
        // last_activity stays whole seconds -- it is a displayed timestamp.
        let last_activity = (freshness.0 / 1_000_000_000) as i64;

        seen.insert(path.to_path_buf());

        if let Some((cached_freshness, cached_session)) = cache.get(path) {
            if *cached_freshness == freshness {
                if let Some(session) = cached_session {
                    out.push(session.clone());
                }
                continue;
            }
        }

        // Cache the outcome either way: most transcripts on a real tree are
        // rejected (sdk sessions, sidechains, no entrypoint), and if we only
        // cached successes, every poll would re-parse all of them forever.
        let session = parse_transcript(path).map(|summary| {
            let cwd = summary.cwd.clone().unwrap_or_default();
            Session {
                session_id: summary.session_id,
                ai_title: summary.ai_title,
                last_prompt: summary.last_prompt,
                git_branch: summary.git_branch,
                project_label: project_label(&cwd),
                cwd,
                version: summary.version,
                last_activity,
                liveness: if summary.interrupted {
                    Liveness::Interrupted
                } else {
                    Liveness::Idle
                },
                annotation: Annotation::default(),
            }
        });

        cache.insert(path.to_path_buf(), (freshness, session.clone()));
        if let Some(session) = session {
            out.push(session);
        }
    }

    // Drop cache entries for transcripts that no longer exist so deleted
    // sessions leave the list rather than lingering forever.
    cache.retain(|path, _| seen.contains(path));

    sort_sessions(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn fixture_tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("-Users-s-code-repo");
        fs::create_dir_all(&proj).unwrap();

        let mut a = fs::File::create(proj.join("aaa.jsonl")).unwrap();
        writeln!(a, r#"{{"type":"user","message":{{"role":"user","content":"hi"}},"entrypoint":"cli","sessionId":"aaa","cwd":"/Users/s/code/repo","gitBranch":"main"}}"#).unwrap();
        writeln!(
            a,
            r#"{{"type":"ai-title","aiTitle":"Session A","sessionId":"aaa"}}"#
        )
        .unwrap();

        let mut b = fs::File::create(proj.join("bbb.jsonl")).unwrap();
        writeln!(b, r#"{{"type":"user","message":{{"role":"user","content":"hi"}},"entrypoint":"sdk-py","sessionId":"bbb","cwd":"/Users/s/code/repo"}}"#).unwrap();

        let wt = dir.path().join("-Users-s-code-repo--worktrees-feat");
        fs::create_dir_all(&wt).unwrap();
        let mut c = fs::File::create(wt.join("ccc.jsonl")).unwrap();
        writeln!(c, r#"{{"type":"user","message":{{"role":"user","content":"hi"}},"entrypoint":"cli","sessionId":"ccc","cwd":"/Users/s/code/repo/worktrees/feat","interruptedByShutdown":true}}"#).unwrap();

        dir
    }

    #[test]
    fn indexes_only_interactive_sessions() {
        let dir = fixture_tree();
        let sessions = index_sessions(dir.path());
        let ids: Vec<&str> = sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert!(ids.contains(&"aaa"));
        assert!(ids.contains(&"ccc"));
        assert!(!ids.contains(&"bbb"), "sdk-py session must be excluded");
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn applies_project_labels_including_worktrees() {
        let dir = fixture_tree();
        let sessions = index_sessions(dir.path());
        let c = sessions.iter().find(|s| s.session_id == "ccc").unwrap();
        assert_eq!(c.project_label, "repo ▸ feat");
        let a = sessions.iter().find(|s| s.session_id == "aaa").unwrap();
        assert_eq!(a.project_label, "repo");
    }

    #[test]
    fn marks_interrupted_sessions() {
        let dir = fixture_tree();
        let sessions = index_sessions(dir.path());
        let c = sessions.iter().find(|s| s.session_id == "ccc").unwrap();
        assert_eq!(c.liveness, Liveness::Interrupted);
        let a = sessions.iter().find(|s| s.session_id == "aaa").unwrap();
        assert_eq!(a.liveness, Liveness::Idle);
    }

    #[test]
    fn sorts_most_recent_first() {
        let dir = fixture_tree();
        let sessions = index_sessions(dir.path());
        for pair in sessions.windows(2) {
            assert!(pair[0].last_activity >= pair[1].last_activity);
        }
    }

    #[test]
    fn missing_root_yields_empty_not_panic() {
        assert!(index_sessions(Path::new("/nonexistent/claudron")).is_empty());
    }

    #[test]
    #[ignore]
    fn real_tree_filters_out_the_vast_majority() {
        let root = projects_root();
        if !root.exists() {
            return;
        }
        let total = WalkDir::new(&root)
            .max_depth(2)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jsonl"))
            .count();
        let sessions = index_sessions(&root);
        println!(
            "transcripts on disk: {}  indexed sessions: {}",
            total,
            sessions.len()
        );
        assert!(
            sessions.len() < total / 2,
            "entrypoint filter should remove most transcripts"
        );
    }

    #[test]
    fn second_scan_of_unchanged_tree_returns_the_same_sessions() {
        let dir = fixture_tree();
        let first = index_sessions(dir.path());
        let second = index_sessions(dir.path());
        assert_eq!(first, second);
        assert!(!first.is_empty());
    }

    #[test]
    fn a_changed_transcript_is_reparsed() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        let path = proj.join("s.jsonl");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"hi"}},"entrypoint":"cli","sessionId":"s","cwd":"/repo"}}"#).unwrap();
        writeln!(
            f,
            r#"{{"type":"ai-title","aiTitle":"First","sessionId":"s"}}"#
        )
        .unwrap();
        drop(f);
        let first = index_sessions(dir.path());
        assert_eq!(first[0].ai_title.as_deref(), Some("First"));

        // Rewrite with a new title and a distinctly newer mtime.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"hi"}},"entrypoint":"cli","sessionId":"s","cwd":"/repo"}}"#).unwrap();
        writeln!(
            f,
            r#"{{"type":"ai-title","aiTitle":"Second","sessionId":"s"}}"#
        )
        .unwrap();
        drop(f);
        let second = index_sessions(dir.path());
        assert_eq!(
            second[0].ai_title.as_deref(),
            Some("Second"),
            "changed file must be re-parsed"
        );
    }

    #[test]
    fn a_transcript_rewritten_within_the_same_second_is_reparsed() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        let path = proj.join("fast.jsonl");

        let write = |title: &str| {
            let mut f = std::fs::File::create(&path).unwrap();
            writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"hi"}},"entrypoint":"cli","sessionId":"fast","cwd":"/repo"}}"#).unwrap();
            writeln!(
                f,
                r#"{{"type":"ai-title","aiTitle":"{title}","sessionId":"fast"}}"#
            )
            .unwrap();
        };

        let secs_of = |p: &std::path::Path| -> u64 {
            std::fs::metadata(p)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0)
        };

        // The premise is that both writes land in the same wall-clock second --
        // otherwise the test would pass even against a seconds-only key, which
        // is a silent flaky-green. Straddling a second boundary is a property of
        // WHEN the test ran, not of the code, so retry rather than fail: a hard
        // assertion here made CI red roughly one run in ten once the suite began
        // looping. Failing loudly after every attempt straddles is still correct.
        let mut straddled = 0;
        loop {
            write("First");
            let secs_first = secs_of(&path);
            let first = index_sessions(dir.path());
            assert_eq!(first[0].ai_title.as_deref(), Some("First"));

            // No sleep: this rewrite lands in the same wall-clock second, which
            // is what an actively-streaming session does constantly.
            write("Secnd");
            let secs_second = secs_of(&path);

            if secs_first == secs_second {
                break;
            }
            straddled += 1;
            assert!(
                straddled < 20,
                "writes straddled a second boundary 20 times running -- the clock \
                 or filesystem is behaving unexpectedly, not a flake"
            );
            // Land the next attempt near the start of a second.
            std::thread::sleep(std::time::Duration::from_millis(120));
        }

        let second = index_sessions(dir.path());
        assert_eq!(
            second[0].ai_title.as_deref(),
            Some("Secnd"),
            "a same-second rewrite must still be re-parsed"
        );
    }

    #[test]
    fn a_deleted_transcript_leaves_the_list() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        let path = proj.join("gone.jsonl");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"hi"}},"entrypoint":"cli","sessionId":"gone","cwd":"/repo"}}"#).unwrap();
        drop(f);
        assert_eq!(index_sessions(dir.path()).len(), 1);
        std::fs::remove_file(&path).unwrap();
        assert!(
            index_sessions(dir.path()).is_empty(),
            "deleted transcript must not linger in cache"
        );
    }

    #[test]
    #[ignore]
    fn warm_scan_of_the_real_tree_is_fast() {
        let root = projects_root();
        if !root.exists() {
            return;
        }
        let t0 = std::time::Instant::now();
        let first = index_sessions(&root);
        let cold = t0.elapsed();
        let t1 = std::time::Instant::now();
        let second = index_sessions(&root);
        let warm = t1.elapsed();
        println!("cold: {cold:?}  warm: {warm:?}  sessions: {}", first.len());
        assert_eq!(first.len(), second.len());
        assert!(
            warm < std::time::Duration::from_secs(2),
            "warm scan took {warm:?}, expected < 2s"
        );
    }

    fn session_with(id: &str, liveness: Liveness, last_activity: i64) -> Session {
        Session {
            session_id: id.into(),
            ai_title: None,
            last_prompt: None,
            git_branch: None,
            cwd: "/repo".into(),
            project_label: "repo".into(),
            version: None,
            last_activity,
            liveness,
            annotation: Annotation::default(),
        }
    }

    #[test]
    fn live_sessions_sort_above_idle_ones_however_old() {
        // A session running right now must lead, even if it was started long
        // before an idle session that was touched seconds ago.
        let mut v = vec![
            session_with("idle-recent", Liveness::Idle, 9_000),
            session_with("live-old", Liveness::Legacy, 1),
        ];
        sort_sessions(&mut v);
        assert_eq!(v[0].session_id, "live-old");
    }

    #[test]
    fn interrupted_sorts_between_live_and_idle() {
        let mut v = vec![
            session_with("idle", Liveness::Idle, 9_000),
            session_with("interrupted", Liveness::Interrupted, 8_000),
            session_with("live", Liveness::Legacy, 7_000),
        ];
        sort_sessions(&mut v);
        let order: Vec<&str> = v.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(order, vec!["live", "interrupted", "idle"]);
    }

    #[test]
    fn most_recent_first_within_a_rank() {
        let mut v = vec![
            session_with("older", Liveness::Idle, 100),
            session_with("newer", Liveness::Idle, 900),
        ];
        sort_sessions(&mut v);
        assert_eq!(v[0].session_id, "newer");
    }

    #[test]
    fn managed_and_legacy_tie_so_activity_decides() {
        // Fixture deliberately puts the OLDER item on Managed. If Managed
        // ranked better than Legacy, rank alone would put it first and this
        // assertion would fail -- so passing proves the two genuinely tie and
        // activity is what decides.
        let mut v = vec![
            session_with("managed-older", Liveness::Managed, 100),
            session_with("legacy-newer", Liveness::Legacy, 900),
        ];
        sort_sessions(&mut v);
        assert_eq!(v[0].session_id, "legacy-newer");

        // And the mirror, so neither variant is favoured in either direction.
        let mut v = vec![
            session_with("legacy-older", Liveness::Legacy, 100),
            session_with("managed-newer", Liveness::Managed, 900),
        ];
        sort_sessions(&mut v);
        assert_eq!(v[0].session_id, "managed-newer");
    }

    #[test]
    fn identical_rank_and_activity_break_ties_deterministically() {
        let mut v = vec![
            session_with("zebra", Liveness::Legacy, 500),
            session_with("alpha", Liveness::Legacy, 500),
        ];
        sort_sessions(&mut v);
        assert_eq!(v[0].session_id, "alpha");

        // Reversed input must produce the same output.
        let mut v = vec![
            session_with("alpha", Liveness::Legacy, 500),
            session_with("zebra", Liveness::Legacy, 500),
        ];
        sort_sessions(&mut v);
        assert_eq!(v[0].session_id, "alpha");
    }
}
