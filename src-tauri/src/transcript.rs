use chrono::{DateTime, NaiveDate, TimeZone};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// The LOCAL calendar date an ISO-8601 timestamp falls on, in `tz`.
///
/// The timezone is a PARAMETER rather than read from the environment so this
/// can be tested at a fixed offset. `cargo test` runs this crate's tests
/// concurrently in one process, and setting `TZ` to pin a zone would race every
/// other test -- the same failure mode that made a `PATH`-clearing test fail
/// about one run in seven.
fn local_date<Tz: TimeZone>(ts: &str, tz: &Tz) -> Option<NaiveDate> {
    let dt = DateTime::parse_from_rfc3339(ts).ok()?;
    Some(dt.with_timezone(tz).date_naive())
}

/// Token and activity totals for ONE local calendar day.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStats {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub tool_calls: u32,
    pub turns: u32,
}

/// What a transcript DID, as opposed to what it is.
///
/// Produced for every transcript, including ones that are not sessions:
/// subagent transcripts carry a large share of the tool calls (measured at
/// 16.4% of all `tool_use` blocks on a real tree) and must contribute their
/// activity even though they never appear as a session.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptStats {
    pub first_ts: Option<String>,
    pub last_ts: Option<String>,
    /// Sum of gaps between consecutive turns that are BELOW `IDLE_GAP`.
    ///
    /// Distinct from `last_ts - first_ts`: 23% of transcripts on a real tree
    /// span more than one calendar day, the longest 186 hours, because a
    /// session resumed days later is still one session. Span answers "when",
    /// this answers "how long".
    pub active_seconds: u64,
    pub turns: u32,
    /// Keyed by LOCAL calendar date -- see `local_date`.
    pub daily: BTreeMap<NaiveDate, DayStats>,
    /// Full tool names, including `mcp__plugin_github_github__issue_read`.
    pub tools: BTreeMap<String, u32>,
    pub models: BTreeMap<String, u32>,
}

/// A transcript's two separable answers: whether it is a session, and what it did.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTranscript {
    /// None when this transcript is not a session: sidechain, non-cli, or no turns.
    pub summary: Option<TranscriptSummary>,
    /// Always produced, even when `summary` is None.
    pub stats: TranscriptStats,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TranscriptSummary {
    pub session_id: String,
    pub ai_title: Option<String>,
    pub last_prompt: Option<String>,
    pub git_branch: Option<String>,
    pub cwd: Option<String>,
    pub version: Option<String>,
    pub interrupted: bool,
}

pub fn parse_transcript<Tz: TimeZone>(path: &Path, tz: &Tz) -> Option<ParsedTranscript> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut summary = TranscriptSummary::default();
    let mut stats = TranscriptStats::default();
    let mut is_cli = false;
    let mut has_conversation = false;
    let mut saw_sidechain = false;
    let mut prev_turn: Option<DateTime<chrono::FixedOffset>> = None;

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        // A sidechain record anywhere disqualifies the transcript as a SESSION,
        // but not as a source of statistics. This used to `return None`, which
        // measured as bailing on line 1 of every subagent transcript -- cheap,
        // but it hid 16.4% of all tool calls from anything built on the index.
        // Reading them through costs 4.1% more bytes on the scan.
        if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            saw_sidechain = true;
        }

        accumulate_stats(&v, tz, &mut stats, &mut prev_turn);

        if let Some(ep) = v.get("entrypoint").and_then(Value::as_str) {
            if ep == "cli" {
                is_cli = true;
            }
        }

        if let Some(id) = v.get("sessionId").and_then(Value::as_str) {
            if summary.session_id.is_empty() {
                summary.session_id = id.to_string();
            }
        }

        if !has_conversation && record_is_a_conversation_turn(&v) {
            has_conversation = true;
        }

        // `ai-title` and `last-prompt` repeat; the last one wins. A
        // `last-prompt` record may carry only a leafUuid and no prompt --
        // those must not overwrite a real value.
        if let Some(t) = v.get("aiTitle").and_then(Value::as_str) {
            summary.ai_title = Some(t.to_string());
        }
        if let Some(p) = v.get("lastPrompt").and_then(Value::as_str) {
            summary.last_prompt = Some(p.to_string());
        }
        if let Some(b) = v.get("gitBranch").and_then(Value::as_str) {
            summary.git_branch = Some(b.to_string());
        }
        if let Some(c) = v.get("cwd").and_then(Value::as_str) {
            summary.cwd = Some(c.to_string());
        }
        if let Some(ver) = v.get("version").and_then(Value::as_str) {
            summary.version = Some(ver.to_string());
        }
        if v.get("interruptedByShutdown").and_then(Value::as_bool) == Some(true) {
            summary.interrupted = true;
        }
    }

    // A launched-but-unused session is not a session. Claude Code writes a
    // transcript when it starts, before anything is typed: entrypoint "cli", a
    // real session id, and nothing but metadata records. Indexed, those render
    // as an empty "Untitled session" row, since aiTitle is absent too.
    //
    // This is safe against the index's rejection cache. That cache is keyed on
    // (mtime, size), so the moment the user types, both move and the rejection
    // is discarded -- the session appears as soon as it becomes one.
    let is_session = is_cli && !summary.session_id.is_empty() && has_conversation && !saw_sidechain;
    Some(ParsedTranscript {
        summary: is_session.then_some(summary),
        stats,
    })
}

/// Gap between consecutive turns beyond which a session is considered idle
/// rather than working.
///
/// Load-bearing for `active_seconds`. A session resumed the next morning has a
/// gap of hours; a user reading output has a gap of seconds. Five minutes sits
/// well clear of both, and the boundary itself is asserted in the tests rather
/// than left to chance.
const IDLE_GAP: i64 = 5 * 60;

/// Fold one transcript record into the running statistics.
///
/// Reads the `Value` the caller has ALREADY built. The scan parses every line
/// of every transcript and discards the result, so these are field lookups on
/// data that is in memory either way -- the reason this feature is affordable
/// at all.
fn accumulate_stats<Tz: TimeZone>(
    v: &Value,
    tz: &Tz,
    stats: &mut TranscriptStats,
    prev_turn: &mut Option<DateTime<chrono::FixedOffset>>,
) {
    let ts = v.get("timestamp").and_then(Value::as_str);
    if let Some(ts) = ts {
        if stats.first_ts.is_none() {
            stats.first_ts = Some(ts.to_string());
        }
        stats.last_ts = Some(ts.to_string());
    }
    let day = ts.and_then(|t| local_date(t, tz));

    if record_is_a_conversation_turn(v) {
        stats.turns += 1;
        if let Some(d) = day {
            stats.daily.entry(d).or_default().turns += 1;
        }
        if let Some(now) = ts.and_then(|t| DateTime::parse_from_rfc3339(t).ok()) {
            if let Some(prev) = *prev_turn {
                let gap = (now - prev).num_seconds();
                if (0..IDLE_GAP).contains(&gap) {
                    stats.active_seconds += gap as u64;
                }
            }
            *prev_turn = Some(now);
        }
    }

    let Some(msg) = v.get("message") else { return };

    if let Some(u) = msg.get("usage") {
        let get = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
        if let Some(d) = day {
            let e = stats.daily.entry(d).or_default();
            e.input_tokens += get("input_tokens");
            e.output_tokens += get("output_tokens");
            e.cache_read_tokens += get("cache_read_input_tokens");
        }
    }

    if let Some(m) = msg.get("model").and_then(Value::as_str) {
        *stats.models.entry(m.to_string()).or_default() += 1;
    }

    if let Some(blocks) = msg.get("content").and_then(Value::as_array) {
        let calls = blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
            .filter_map(|b| b.get("name").and_then(Value::as_str));
        let mut n = 0u32;
        for name in calls {
            *stats.tools.entry(name.to_string()).or_default() += 1;
            n += 1;
        }
        if n > 0 {
            if let Some(d) = day {
                stats.daily.entry(d).or_default().tool_calls += n;
            }
        }
    }
}

/// Whether a record is an actual conversation turn rather than metadata.
///
/// A turn is a `user` or `assistant` record carrying non-empty message content.
/// Everything else a transcript contains -- `last-prompt`, `mode`,
/// `permission-mode`, `system`, `file-history-snapshot`, `queue-operation`,
/// `attachment` -- is bookkeeping that exists before and around the
/// conversation, and none of it means the session was used.
fn record_is_a_conversation_turn(v: &Value) -> bool {
    let is_turn = matches!(
        v.get("type").and_then(Value::as_str),
        Some("user") | Some("assistant")
    );
    if !is_turn {
        return false;
    }

    // Content is a string for a plain user prompt and an array of blocks for an
    // assistant reply. Either is a turn; an empty array is a placeholder.
    match v.get("message").and_then(|m| m.get("content")) {
        Some(Value::String(s)) => !s.trim().is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;
    use std::io::Write;

    /// UTC-4, the offset that makes the local/UTC date disagree for evening work.
    fn minus_four() -> FixedOffset {
        FixedOffset::east_opt(-4 * 3600).unwrap()
    }

    /// The session half of a parse, for tests that are only about rejection
    /// rules. Fixed at one offset so day bucketing cannot vary by machine.
    fn summary(path: &Path) -> Option<TranscriptSummary> {
        parse_transcript(path, &minus_four()).and_then(|p| p.summary)
    }

    /// The stats half.
    fn stats_of(path: &Path) -> TranscriptStats {
        parse_transcript(path, &minus_four())
            .expect("file should be readable")
            .stats
    }

    #[test]
    fn a_transcript_spanning_two_local_days_splits_its_tokens_across_two_buckets() {
        // Measured on a real tree: 23% of transcripts span more than one calendar
        // day, the longest four days. Attributing a session's totals to its start
        // date would misreport every one of them, so this is the case that makes
        // per-day buckets mandatory rather than an optimisation.
        //
        // 18:00Z on the 8th is 14:00 local; 03:30Z on the 10th is 23:30 local on
        // the 9th. Two local days.
        //
        // Every token field holds a DIFFERENT value, so swapping input for output
        // or either for cache-read cannot pass.
        let f = write_jsonl(&[
            r#"{"type":"user","message":{"role":"user","content":"go"},"entrypoint":"cli","sessionId":"s1","timestamp":"2026-08-08T18:00:00.000Z"}"#,
            r#"{"type":"assistant","timestamp":"2026-08-08T18:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"a"}],"usage":{"input_tokens":5,"output_tokens":7,"cache_read_input_tokens":9}}}"#,
            r#"{"type":"assistant","timestamp":"2026-08-10T03:30:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"b"}],"usage":{"input_tokens":11,"output_tokens":13,"cache_read_input_tokens":17}}}"#,
        ]);

        let s = stats_of(f.path());
        let day8 = NaiveDate::from_ymd_opt(2026, 8, 8).unwrap();
        let day9 = NaiveDate::from_ymd_opt(2026, 8, 9).unwrap();

        assert_eq!(
            s.daily.keys().copied().collect::<Vec<_>>(),
            vec![day8, day9],
            "expected one bucket per local day, got {:?}",
            s.daily
        );
        assert_eq!(s.daily[&day8].input_tokens, 5);
        assert_eq!(s.daily[&day8].output_tokens, 7);
        assert_eq!(s.daily[&day8].cache_read_tokens, 9);
        assert_eq!(s.daily[&day9].input_tokens, 11);
        assert_eq!(s.daily[&day9].output_tokens, 13);
        assert_eq!(s.daily[&day9].cache_read_tokens, 17);
    }

    #[test]
    fn a_subagent_transcript_reports_stats_even_though_it_is_not_a_session() {
        // Measured on a real tree: sidechain-containing transcripts hold 16.4% of
        // all tool calls and 5.3% of all tokens, and `parse_transcript` bailed on
        // line 1 of every one of them. Those files must still contribute activity
        // while never becoming a session.
        //
        // This fails both ways round: against the old early return (no stats at
        // all) and against any implementation that lets a subagent through as a
        // session.
        let f = write_jsonl(&[
            r#"{"type":"assistant","isSidechain":true,"timestamp":"2026-08-08T18:00:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Grep","input":{}}],"usage":{"input_tokens":3,"output_tokens":4,"cache_read_input_tokens":5}}}"#,
        ]);

        let parsed = parse_transcript(f.path(), &minus_four()).expect("file should be readable");
        assert!(
            parsed.summary.is_none(),
            "a sidechain transcript must never be a session"
        );
        let day = NaiveDate::from_ymd_opt(2026, 8, 8).unwrap();
        assert_eq!(
            parsed.stats.daily.get(&day).map(|d| d.cache_read_tokens),
            Some(5),
            "stats must survive the rejection, got {:?}",
            parsed.stats
        );
    }

    #[test]
    fn active_seconds_counts_working_time_not_the_span_of_a_resumed_session() {
        // The longest transcript on a real tree spans 186 hours across four days.
        // Reporting that as "session duration" describes a session resumed over a
        // long weekend, not four days of work.
        //
        // Two bursts, 30s and 60s of activity, separated by two days. Anything
        // computing `last_ts - first_ts` answers ~172800s and fails here.
        let f = write_jsonl(&[
            r#"{"type":"user","message":{"role":"user","content":"a"},"timestamp":"2026-08-08T18:00:00.000Z"}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"b"}]},"timestamp":"2026-08-08T18:00:30.000Z"}"#,
            r#"{"type":"user","message":{"role":"user","content":"c"},"timestamp":"2026-08-10T18:00:00.000Z"}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"d"}]},"timestamp":"2026-08-10T18:01:00.000Z"}"#,
        ]);

        let s = stats_of(f.path());
        assert_eq!(
            s.active_seconds, 90,
            "30s + 60s of work, not the two-day gap between them"
        );
    }

    #[test]
    fn a_gap_exactly_at_the_idle_threshold_is_idle_not_working() {
        // Pins which side of IDLE_GAP the boundary falls on. Without this the
        // constant could be changed from `<` to `<=` with no test objecting, and
        // every duration figure would shift.
        let f = write_jsonl(&[
            r#"{"type":"user","message":{"role":"user","content":"a"},"timestamp":"2026-08-08T18:00:00.000Z"}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"b"}]},"timestamp":"2026-08-08T18:05:00.000Z"}"#,
        ]);
        assert_eq!(
            stats_of(f.path()).active_seconds,
            0,
            "a gap of exactly IDLE_GAP is idle"
        );
    }

    #[test]
    fn tool_calls_are_counted_under_their_full_name_including_mcp_names() {
        // 34 distinct tool names appear on a real tree, and the MCP ones are long
        // and structured (`mcp__plugin_github_github__issue_read`). Truncating or
        // normalising them would silently merge distinct tools in the ranking.
        let f = write_jsonl(&[
            r#"{"type":"assistant","timestamp":"2026-08-08T18:00:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}},{"type":"tool_use","id":"t2","name":"Bash","input":{}},{"type":"tool_use","id":"t3","name":"mcp__plugin_github_github__issue_read","input":{}}]}}"#,
        ]);

        let s = stats_of(f.path());
        assert_eq!(s.tools.get("Bash").copied(), Some(2));
        assert_eq!(
            s.tools
                .get("mcp__plugin_github_github__issue_read")
                .copied(),
            Some(1),
            "full MCP tool name must survive verbatim, got {:?}",
            s.tools
        );
    }

    #[test]
    fn a_late_evening_timestamp_buckets_to_the_local_date_not_the_utc_date() {
        // 03:30 UTC on the 9th is 23:30 on the 8th at UTC-4. Measured on the real
        // tree, every recorded timestamp happened to agree between UTC and local
        // -- purely because all activity fell in working hours. One evening
        // session breaks that, so this is the case the design exists for.
        //
        // An implementation that slices the first ten characters of the ISO
        // string answers the 9th and fails here.
        let d = local_date("2026-08-09T03:30:00.000Z", &minus_four()).expect("should parse");
        assert_eq!(
            d,
            NaiveDate::from_ymd_opt(2026, 8, 8).unwrap(),
            "23:30 local on the 8th must bucket to the 8th"
        );
    }

    fn write_jsonl(lines: &[&str]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for l in lines {
            writeln!(f, "{}", l).unwrap();
        }
        f.flush().unwrap();
        f
    }

    #[test]
    fn parses_a_cli_session() {
        let f = write_jsonl(&[
            r#"{"type":"user","message":{"role":"user","content":"hi"},"entrypoint":"cli","sessionId":"s1","cwd":"/repo","gitBranch":"main","version":"2.1.220","isSidechain":false}"#,
            r#"{"type":"ai-title","aiTitle":"Fix the parser","sessionId":"s1"}"#,
            r#"{"type":"last-prompt","lastPrompt":"do the thing","sessionId":"s1"}"#,
        ]);
        let s = summary(f.path()).expect("should parse");
        assert_eq!(s.session_id, "s1");
        assert_eq!(s.ai_title.as_deref(), Some("Fix the parser"));
        assert_eq!(s.last_prompt.as_deref(), Some("do the thing"));
        assert_eq!(s.git_branch.as_deref(), Some("main"));
        assert_eq!(s.cwd.as_deref(), Some("/repo"));
        assert!(!s.interrupted);
    }

    #[test]
    fn rejects_sdk_sessions() {
        let f = write_jsonl(&[
            r#"{"type":"user","entrypoint":"sdk-py","sessionId":"s2","cwd":"/repo"}"#,
        ]);
        assert!(summary(f.path()).is_none());
    }

    #[test]
    fn rejects_transcripts_with_no_entrypoint() {
        let f = write_jsonl(&[r#"{"type":"user","sessionId":"s3","cwd":"/repo"}"#]);
        assert!(summary(f.path()).is_none());
    }

    #[test]
    fn rejects_a_launched_but_unused_session() {
        // Claude Code writes a transcript at LAUNCH, before any conversation.
        // Such a file carries entrypoint "cli" and a session id but contains only
        // metadata records -- no user or assistant message. Two of these existed
        // on the machine this was found on, each ~20KB, and both rendered as an
        // empty "Untitled session" row because aiTitle is absent too.
        let f = write_jsonl(&[
            r#"{"type":"last-prompt","entrypoint":"cli","sessionId":"s9","cwd":"/repo","leafUuid":"u1"}"#,
            r#"{"type":"mode","sessionId":"s9"}"#,
            r#"{"type":"permission-mode","sessionId":"s9"}"#,
            r#"{"type":"system","sessionId":"s9"}"#,
            r#"{"type":"file-history-snapshot","sessionId":"s9"}"#,
        ]);
        assert!(
            summary(f.path()).is_none(),
            "a transcript with no conversation must not be indexed"
        );
    }

    #[test]
    fn accepts_a_session_as_soon_as_it_has_one_real_turn() {
        // The counterpart: the moment the user types, the session must appear.
        // Rejecting on "no conversation" must not reject on "not much yet".
        let f = write_jsonl(&[
            r#"{"type":"last-prompt","entrypoint":"cli","sessionId":"s10","cwd":"/repo","leafUuid":"u1"}"#,
            r#"{"type":"user","sessionId":"s10","message":{"role":"user","content":"hello"}}"#,
        ]);
        let got = summary(f.path()).expect("one real turn is enough");
        assert_eq!(got.session_id, "s10");
    }

    #[test]
    fn an_assistant_turn_alone_also_counts_as_a_conversation() {
        let f = write_jsonl(&[
            r#"{"type":"assistant","entrypoint":"cli","sessionId":"s11","cwd":"/repo","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
        ]);
        assert!(summary(f.path()).is_some());
    }

    #[test]
    fn a_message_with_empty_content_is_not_a_conversation() {
        // An empty content array is a structural placeholder, not a turn.
        let f = write_jsonl(&[
            r#"{"type":"user","entrypoint":"cli","sessionId":"s12","cwd":"/repo","message":{"role":"user","content":[]}}"#,
        ]);
        assert!(summary(f.path()).is_none());
    }

    #[test]
    fn rejects_sidechains() {
        let f = write_jsonl(&[
            r#"{"type":"user","message":{"role":"user","content":"hi"},"entrypoint":"cli","sessionId":"s4","cwd":"/repo","isSidechain":true}"#,
        ]);
        assert!(summary(f.path()).is_none());
    }

    #[test]
    fn takes_the_last_title_and_prompt() {
        let f = write_jsonl(&[
            r#"{"type":"user","message":{"role":"user","content":"hi"},"entrypoint":"cli","sessionId":"s5","cwd":"/repo"}"#,
            r#"{"type":"ai-title","aiTitle":"First title","sessionId":"s5"}"#,
            r#"{"type":"last-prompt","lastPrompt":"first prompt","sessionId":"s5"}"#,
            r#"{"type":"ai-title","aiTitle":"Second title","sessionId":"s5"}"#,
            r#"{"type":"last-prompt","lastPrompt":"second prompt","sessionId":"s5"}"#,
        ]);
        let s = summary(f.path()).unwrap();
        assert_eq!(s.ai_title.as_deref(), Some("Second title"));
        assert_eq!(s.last_prompt.as_deref(), Some("second prompt"));
    }

    #[test]
    fn ignores_last_prompt_records_without_a_prompt_field() {
        let f = write_jsonl(&[
            r#"{"type":"user","message":{"role":"user","content":"hi"},"entrypoint":"cli","sessionId":"s6","cwd":"/repo"}"#,
            r#"{"type":"last-prompt","lastPrompt":"real prompt","sessionId":"s6"}"#,
            r#"{"type":"last-prompt","leafUuid":"abc","sessionId":"s6"}"#,
        ]);
        let s = summary(f.path()).unwrap();
        assert_eq!(s.last_prompt.as_deref(), Some("real prompt"));
    }

    #[test]
    fn detects_interruption() {
        let f = write_jsonl(&[
            r#"{"type":"user","message":{"role":"user","content":"hi"},"entrypoint":"cli","sessionId":"s7","cwd":"/repo","interruptedByShutdown":true}"#,
        ]);
        assert!(summary(f.path()).unwrap().interrupted);
    }

    #[test]
    fn skips_malformed_lines_without_failing() {
        let f = write_jsonl(&[
            r#"{"type":"user","message":{"role":"user","content":"hi"},"entrypoint":"cli","sessionId":"s8","cwd":"/repo"}"#,
            r#"this is not json at all"#,
            r#"{"type":"ai-title","aiTitle":"Survived","sessionId":"s8"}"#,
        ]);
        let s = summary(f.path()).unwrap();
        assert_eq!(s.ai_title.as_deref(), Some("Survived"));
    }

    #[test]
    fn returns_none_for_missing_file() {
        assert!(summary(Path::new("/nonexistent/x.jsonl")).is_none());
    }
}
