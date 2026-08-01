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

/// How far through a scan we are, measured in BYTES rather than files.
///
/// File count is a dishonest denominator here: measured on a real tree, the ten
/// largest transcripts are 52.7% of total parse time, so a count-based bar
/// races to ~99% and then stalls for seconds on a handful of files. Those same
/// ten files are 47.8% of total bytes, so bytes track the work almost exactly.
///
/// The denominator is free: enumerating 1476 files took 68 ms against 11.8 s of
/// parsing, and `metadata().len()` is already read for the freshness key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

impl ScanProgress {
    /// Fraction complete in 0.0..=1.0. Zero total is complete, not divide-by-zero.
    pub fn fraction(&self) -> f64 {
        if self.bytes_total == 0 {
            return 1.0;
        }
        (self.bytes_done as f64 / self.bytes_total as f64).clamp(0.0, 1.0)
    }
}

/// Whether enough progress has been made to be worth reporting.
///
/// Reports at roughly every 1% of total bytes. Per-file reporting would emit
/// ~1476 events for one scan, and each one crosses the Tauri IPC boundary and
/// re-renders React -- so the reporting would measurably slow the very scan it
/// describes. 1% is far below what the eye resolves on a progress bar.
fn should_report(bytes_done: u64, last_reported: u64, bytes_total: u64) -> bool {
    if bytes_total == 0 {
        return false;
    }
    let step = (bytes_total / 100).max(1);
    bytes_done.saturating_sub(last_reported) >= step
}

pub fn index_sessions(root: &Path) -> Vec<Session> {
    index_sessions_with_progress(root, |_| {})
}

/// `index_sessions`, reporting progress as it goes.
///
/// `on_progress` is called as files are processed, so a caller can drive a
/// progress bar. It is NOT called per file: at 1476 files that would be 1476
/// events for an 11.8 s scan, most of them redundant. See `should_report`.
pub fn index_sessions_with_progress(
    root: &Path,
    mut on_progress: impl FnMut(ScanProgress),
) -> Vec<Session> {
    let mut out = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());

    // Enumerate first so the denominator is known before any parsing. Measured
    // at 68 ms for 1476 files against 11.8 s of parsing -- cheap enough that
    // the honest progress bar costs essentially nothing.
    let files: Vec<_> = WalkDir::new(root)
        .max_depth(2)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jsonl"))
        .collect();

    let files_total = files.len();
    let bytes_total: u64 = files
        .iter()
        .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
        .sum();
    let mut files_done = 0usize;
    let mut bytes_done = 0u64;
    let mut last_reported = 0u64;

    on_progress(ScanProgress {
        files_done: 0,
        files_total,
        bytes_done: 0,
        bytes_total,
    });

    for entry in files {
        let path = entry.path();

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
                // A cache hit is still work done. Counting only the parse path
                // would leave a warm scan reporting 0% forever -- and a warm
                // scan is the common case, at ~19ms against 11.8s cold.
                files_done += 1;
                bytes_done += freshness.1;
                if should_report(bytes_done, last_reported, bytes_total) {
                    last_reported = bytes_done;
                    on_progress(ScanProgress {
                        files_done,
                        files_total,
                        bytes_done,
                        bytes_total,
                    });
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

        files_done += 1;
        bytes_done += freshness.1;
        if should_report(bytes_done, last_reported, bytes_total) {
            last_reported = bytes_done;
            on_progress(ScanProgress {
                files_done,
                files_total,
                bytes_done,
                bytes_total,
            });
        }
    }

    // A final report, so a caller always sees 100% even when the last files
    // fell inside the reporting threshold.
    on_progress(ScanProgress {
        files_done,
        files_total,
        bytes_done,
        bytes_total,
    });

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
    fn progress_is_measured_in_bytes_not_files() {
        // File count is dishonest here: measured on a real tree, the ten
        // largest transcripts are 52.7% of parse time but only 0.7% of the
        // file count. A count-based bar races to 99% then stalls for seconds.
        let p = ScanProgress {
            files_done: 99,
            files_total: 100,
            bytes_done: 10,
            bytes_total: 100,
        };
        assert!(
            (p.fraction() - 0.10).abs() < f64::EPSILON,
            "fraction must follow bytes (10%), not files (99%)"
        );
    }

    #[test]
    fn an_empty_tree_is_complete_not_a_divide_by_zero() {
        let p = ScanProgress {
            files_done: 0,
            files_total: 0,
            bytes_done: 0,
            bytes_total: 0,
        };
        assert_eq!(p.fraction(), 1.0);
    }

    #[test]
    fn fraction_never_exceeds_one() {
        // Files can grow mid-scan: a live session writes while we read it.
        let p = ScanProgress {
            files_done: 5,
            files_total: 5,
            bytes_done: 500,
            bytes_total: 100,
        };
        assert_eq!(
            p.fraction(),
            1.0,
            "a growing file must not push the bar past 100%"
        );
    }

    #[test]
    fn reporting_is_throttled_to_about_one_percent() {
        // Per-file reporting would emit ~1476 events for one scan, each
        // crossing the IPC boundary and re-rendering React -- slowing the very
        // scan it describes.
        let total = 1_000_000;
        assert!(
            !should_report(5_000, 0, total),
            "0.5% is not worth an event"
        );
        assert!(should_report(10_000, 0, total), "1% is");
        assert!(
            !should_report(19_000, 10_000, total),
            "measured from the last report"
        );
        assert!(should_report(20_000, 10_000, total));
    }

    #[test]
    fn a_zero_byte_total_never_reports() {
        assert!(!should_report(0, 0, 0), "must not divide by zero or spam");
    }

    #[test]
    fn a_scan_reports_progress_that_starts_at_zero_and_ends_complete() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        for i in 0..3 {
            let mut f = std::fs::File::create(proj.join(format!("s{i}.jsonl"))).unwrap();
            writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"hi"}},"entrypoint":"cli","sessionId":"s{i}","cwd":"/repo"}}"#).unwrap();
        }

        let mut seen: Vec<ScanProgress> = Vec::new();
        let out = index_sessions_with_progress(dir.path(), |p| seen.push(p));

        assert_eq!(out.len(), 3);
        assert!(
            seen.len() >= 2,
            "expected at least a first and final report"
        );
        assert_eq!(
            seen[0].bytes_done, 0,
            "the first report must be 0%, not partial"
        );
        assert_eq!(
            seen[0].files_total, 3,
            "the denominator must be known up front"
        );

        let last = seen.last().unwrap();
        assert_eq!(last.files_done, 3);
        assert_eq!(
            last.bytes_done, last.bytes_total,
            "the final report must be 100%"
        );
        assert_eq!(last.fraction(), 1.0);
    }

    #[test]
    fn a_warm_scan_still_reaches_one_hundred_percent() {
        // The cache-hit path `continue`s. If progress is only counted on the
        // parse path, a second (warm) scan reports 0 of N bytes done forever --
        // a bar that never moves is worse than no bar.
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        for i in 0..3 {
            let mut f = std::fs::File::create(proj.join(format!("warm{i}.jsonl"))).unwrap();
            writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"hi"}},"entrypoint":"cli","sessionId":"warm{i}","cwd":"/repo"}}"#).unwrap();
        }

        index_sessions(dir.path()); // populate the cache

        let mut seen: Vec<ScanProgress> = Vec::new();
        index_sessions_with_progress(dir.path(), |p| seen.push(p));
        let last = seen.last().expect("a final report");
        assert_eq!(last.files_done, 3, "cached files must still count as done");
        assert_eq!(
            last.bytes_done, last.bytes_total,
            "a warm scan must still finish at 100%, not 0%"
        );
    }

    #[test]
    fn progress_never_goes_backwards() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        for i in 0..5 {
            let mut f = std::fs::File::create(proj.join(format!("s{i}.jsonl"))).unwrap();
            writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":"hi"}},"entrypoint":"cli","sessionId":"s{i}","cwd":"/repo"}}"#).unwrap();
        }
        let mut seen: Vec<ScanProgress> = Vec::new();
        index_sessions_with_progress(dir.path(), |p| seen.push(p));
        for w in seen.windows(2) {
            assert!(
                w[1].bytes_done >= w[0].bytes_done,
                "bytes went backwards: {:?}",
                w
            );
            assert!(
                w[1].files_done >= w[0].files_done,
                "files went backwards: {:?}",
                w
            );
        }
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
