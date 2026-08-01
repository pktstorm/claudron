use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Event files older than this are deleted on read.
///
/// A session that crashes never fires `SessionEnd`, so its file would otherwise
/// live forever -- and pids are reused, so a stale file eventually names a pid
/// belonging to something else entirely. Liveness is always confirmed against
/// the live process list before a file is believed, so a stale entry cannot
/// produce a false "live"; this only stops the directory growing without bound.
const STALE_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// One hook event, as written by the hook script.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookEvent {
    /// The `claude` process that fired this event -- the hook's `$PPID`.
    pub pid: i32,
    pub session_id: String,
    pub cwd: String,
    /// The lifecycle event name, e.g. `SessionStart`.
    pub event: String,
    /// Unix seconds, written by the hook.
    pub ts: u64,
}

/// Parse one event file's contents.
///
/// Separate from the filesystem walk so the shapes a hook can actually produce
/// -- including a truncated write from a session killed mid-hook -- are
/// testable without staging files on disk.
pub fn parse_event(contents: &str) -> Option<HookEvent> {
    let v: serde_json::Value = serde_json::from_str(contents.trim()).ok()?;
    let pid = v.get("pid").and_then(serde_json::Value::as_i64)? as i32;
    let session_id = v.get("session_id").and_then(serde_json::Value::as_str)?;
    // A pid or session id that is absent, empty, or non-numeric means the hook
    // wrote something we cannot key on. Better no entry than a wrong one.
    if pid <= 0 || session_id.is_empty() {
        return None;
    }
    Some(HookEvent {
        pid,
        session_id: session_id.to_string(),
        cwd: v
            .get("cwd")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string(),
        event: v
            .get("event")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string(),
        ts: v.get("ts").and_then(serde_json::Value::as_u64).unwrap_or(0),
    })
}

/// Read every event file, keyed by session id.
///
/// Keyed by session id rather than pid because that is what callers join on --
/// the pid is carried inside so liveness can be confirmed against the live
/// process list. A missing directory is the normal no-hooks-installed case and
/// yields an empty map, never an error.
pub fn read_events(dir: &Path) -> HashMap<String, HookEvent> {
    let mut out = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };

    let now = SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        if let Ok(meta) = entry.metadata() {
            if let Ok(modified) = meta.modified() {
                if now.duration_since(modified).unwrap_or_default() > STALE_AFTER {
                    let _ = std::fs::remove_file(&path);
                    continue;
                }
            }
        }

        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(event) = parse_event(&contents) else {
            continue;
        };

        // Two files can name the same session if a pid was reused after a
        // crash. Keep the most recent; the older one is necessarily stale.
        match out.get(&event.session_id) {
            Some(existing) if existing.ts >= event.ts => {}
            _ => {
                out.insert(event.session_id.clone(), event);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// The exact shape the hook script writes.
    const REAL: &str = r#"{"event":"SessionStart","pid":83463,"session_id":"20f99a24-c1ab-4f4e-a8b7-5550f631f456","cwd":"/Users/s/code/repo","ts":1785546426}"#;

    #[test]
    fn parses_a_real_event() {
        let e = parse_event(REAL).expect("should parse");
        assert_eq!(e.pid, 83463);
        assert_eq!(e.session_id, "20f99a24-c1ab-4f4e-a8b7-5550f631f456");
        assert_eq!(e.cwd, "/Users/s/code/repo");
        assert_eq!(e.event, "SessionStart");
        assert_eq!(e.ts, 1785546426);
    }

    #[test]
    fn a_truncated_write_is_rejected_not_panicked() {
        // A session killed mid-hook leaves a partial file.
        assert!(parse_event(r#"{"event":"SessionStart","pid":834"#).is_none());
        assert!(parse_event("").is_none());
        assert!(parse_event("not json at all").is_none());
    }

    #[test]
    fn an_event_without_a_usable_pid_or_session_is_rejected() {
        // Keying on either of these is the whole point; without one there is
        // nothing to join on and a wrong key is worse than no entry.
        assert!(parse_event(r#"{"session_id":"s1","cwd":"/x"}"#).is_none());
        assert!(parse_event(r#"{"pid":0,"session_id":"s1"}"#).is_none());
        assert!(parse_event(r#"{"pid":123,"session_id":""}"#).is_none());
        assert!(parse_event(r#"{"pid":"not-a-number","session_id":"s1"}"#).is_none());
    }

    #[test]
    fn missing_optional_fields_still_parse() {
        let e = parse_event(r#"{"pid":42,"session_id":"s1"}"#).expect("pid+session is enough");
        assert_eq!(e.cwd, "");
        assert_eq!(e.ts, 0);
    }

    fn write_event(dir: &Path, name: &str, body: &str) {
        let mut f = std::fs::File::create(dir.join(name)).unwrap();
        f.write_all(body.as_bytes()).unwrap();
    }

    #[test]
    fn a_missing_directory_is_empty_not_an_error() {
        // The normal case before hooks are installed.
        let got = read_events(Path::new("/tmp/claudron-no-such-dir-zzz"));
        assert!(got.is_empty());
    }

    #[test]
    fn reads_events_keyed_by_session_id() {
        let d = tempfile::tempdir().unwrap();
        write_event(d.path(), "111.json", REAL);
        write_event(
            d.path(),
            "222.json",
            r#"{"event":"Stop","pid":222,"session_id":"other","cwd":"/y","ts":50}"#,
        );
        let got = read_events(d.path());
        assert_eq!(got.len(), 2);
        assert_eq!(got["other"].pid, 222);
        assert_eq!(got["20f99a24-c1ab-4f4e-a8b7-5550f631f456"].pid, 83463);
    }

    #[test]
    fn a_corrupt_file_does_not_discard_the_good_ones() {
        let d = tempfile::tempdir().unwrap();
        write_event(d.path(), "111.json", REAL);
        write_event(d.path(), "222.json", "{ truncated");
        let got = read_events(d.path());
        assert_eq!(
            got.len(),
            1,
            "the readable event must survive its neighbour"
        );
    }

    #[test]
    fn non_json_files_are_ignored() {
        let d = tempfile::tempdir().unwrap();
        write_event(d.path(), "111.json", REAL);
        write_event(d.path(), "notes.txt", "irrelevant");
        assert_eq!(read_events(d.path()).len(), 1);
    }

    #[test]
    fn the_most_recent_event_wins_when_a_pid_was_reused() {
        // A crashed session leaves its file; a later process reuses the pid and
        // writes its own. Both name the same session only if the id repeats --
        // here two files claim one session, and the newer must win.
        let d = tempfile::tempdir().unwrap();
        write_event(
            d.path(),
            "111.json",
            r#"{"event":"SessionStart","pid":111,"session_id":"s","cwd":"/old","ts":100}"#,
        );
        write_event(
            d.path(),
            "222.json",
            r#"{"event":"Stop","pid":222,"session_id":"s","cwd":"/new","ts":900}"#,
        );
        let got = read_events(d.path());
        assert_eq!(got.len(), 1);
        assert_eq!(got["s"].cwd, "/new", "newer event must win");
        assert_eq!(got["s"].pid, 222);
    }
}
