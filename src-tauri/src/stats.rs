//! Dashboard aggregates, rolled up from the per-transcript statistics the
//! session scan already produces.
//!
//! Owns no parsing: `transcript::parse_transcript` extracts, this merges.

use crate::model::{Liveness, Session};
use crate::transcript::{DayStats, TranscriptStats};
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One transcript as the index saw it: where it is, whether it is a session, and
/// what it did.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptEntry {
    pub path: PathBuf,
    pub session: Option<Session>,
    pub stats: TranscriptStats,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LivenessCounts {
    pub live: u32,
    pub interrupted: u32,
    pub idle: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenTotals {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    /// `cache_read / (cache_read + input)`, or None when nothing was read in.
    pub hit_rate: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStat {
    pub name: String,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoStat {
    pub label: String,
    pub sessions: u32,
}

/// Percentiles over per-session ACTIVE time, never over wall-clock span.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DurationStats {
    pub median_seconds: u64,
    pub p90_seconds: u64,
    pub max_seconds: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardStats {
    pub sessions_total: u32,
    pub liveness: LivenessCounts,
    pub sessions_by_repo: Vec<RepoStat>,
    pub outdated_versions: u32,
    pub tokens: TokenTotals,
    pub daily: Vec<DayPoint>,
    pub top_tools: Vec<ToolStat>,
    pub duration: DurationStats,
}

/// One point on the dashboard's time-series charts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayPoint {
    pub date: NaiveDate,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub tool_calls: u32,
    pub turns: u32,
}

/// An ordered, GAP-FILLED series from the first day with activity to the last.
///
/// Days with no activity are emitted as zeros rather than omitted. Measured on a
/// real tree, activity ran 2026-07-28 to 08-02 and then skipped 08-03, 08-05,
/// 08-06 and 08-07 entirely; an area chart plotting only the days present
/// compresses those gaps and overstates how continuous the work was.
pub fn daily_series(merged: &BTreeMap<NaiveDate, DayStats>) -> Vec<DayPoint> {
    let (Some(first), Some(last)) = (
        merged.keys().next().copied(),
        merged.keys().next_back().copied(),
    ) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut day = first;
    while day <= last {
        let d = merged.get(&day).cloned().unwrap_or_default();
        out.push(DayPoint {
            date: day,
            input_tokens: d.input_tokens,
            output_tokens: d.output_tokens,
            cache_read_tokens: d.cache_read_tokens,
            tool_calls: d.tool_calls,
            turns: d.turns,
        });
        day += Duration::days(1);
    }
    out
}

/// The transcript whose session a subagent file's activity belongs to.
///
/// Attribution is by PATH, not by content. `conversation::subagent::subagent_path`
/// builds `<dir>/<stem>/subagents/agent-<id>.jsonl`, so the owning transcript is
/// `<dir>/<stem>.jsonl` -- recoverable without reading either file.
///
/// Returns None for an ordinary transcript, which owns its own activity.
pub fn parent_transcript(path: &Path) -> Option<PathBuf> {
    let subagents = path.parent()?;
    if subagents.file_name()? != "subagents" {
        return None;
    }
    let stem_dir = subagents.parent()?;
    let stem = stem_dir.file_name()?;
    Some(stem_dir.with_file_name(format!("{}.jsonl", stem.to_string_lossy())))
}

/// Merge every transcript's statistics into the dashboard's shape.
///
/// Two invariants the raw data will not give for free:
///
/// 1. Subagent transcripts contribute tokens and tool calls, credited to the
///    session that spawned them, but NEVER add to the session count.
/// 2. The daily series is gap-filled, so quiet days render as zero rather than
///    vanishing.
pub fn rollup(entries: &[TranscriptEntry], baseline: Option<&str>) -> DashboardStats {
    let mut out = DashboardStats::default();
    let mut merged_daily: BTreeMap<NaiveDate, DayStats> = BTreeMap::new();
    let mut tools: BTreeMap<String, u32> = BTreeMap::new();
    let mut repos: BTreeMap<String, u32> = BTreeMap::new();
    // Active time per OWNING transcript, so a session's subagents fold into it.
    let mut active: BTreeMap<PathBuf, u64> = BTreeMap::new();

    for e in entries {
        for (day, d) in &e.stats.daily {
            let acc = merged_daily.entry(*day).or_default();
            acc.input_tokens += d.input_tokens;
            acc.output_tokens += d.output_tokens;
            acc.cache_read_tokens += d.cache_read_tokens;
            acc.tool_calls += d.tool_calls;
            acc.turns += d.turns;
        }
        for (name, n) in &e.stats.tools {
            *tools.entry(name.clone()).or_default() += n;
        }

        let owner = parent_transcript(&e.path).unwrap_or_else(|| e.path.clone());
        *active.entry(owner).or_default() += e.stats.active_seconds;

        let Some(s) = &e.session else { continue };
        out.sessions_total += 1;
        match s.liveness {
            Liveness::Legacy | Liveness::Managed => out.liveness.live += 1,
            Liveness::Interrupted => out.liveness.interrupted += 1,
            Liveness::Idle => out.liveness.idle += 1,
        }
        *repos.entry(s.project_label.clone()).or_default() += 1;
        if let (Some(v), Some(b)) = (s.version.as_deref(), baseline) {
            if crate::version::is_older(v, b) {
                out.outdated_versions += 1;
            }
        }
    }

    out.tokens.input = merged_daily.values().map(|d| d.input_tokens).sum();
    out.tokens.output = merged_daily.values().map(|d| d.output_tokens).sum();
    out.tokens.cache_read = merged_daily.values().map(|d| d.cache_read_tokens).sum();
    let read_in = out.tokens.cache_read + out.tokens.input;
    out.tokens.hit_rate = (read_in > 0).then(|| out.tokens.cache_read as f64 / read_in as f64);

    out.daily = daily_series(&merged_daily);

    out.top_tools = {
        let mut v: Vec<ToolStat> = tools
            .into_iter()
            .map(|(name, count)| ToolStat { name, count })
            .collect();
        // Ties break on name so the ranking is stable between polls rather than
        // reordering rows under the reader.
        v.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
        v
    };

    out.sessions_by_repo = {
        let mut v: Vec<RepoStat> = repos
            .into_iter()
            .map(|(label, sessions)| RepoStat { label, sessions })
            .collect();
        v.sort_by(|a, b| b.sessions.cmp(&a.sessions).then(a.label.cmp(&b.label)));
        v
    };

    // Only transcripts that ARE sessions get a duration; a subagent's time is
    // already folded into its parent above.
    let session_paths: Vec<&PathBuf> = entries
        .iter()
        .filter(|e| e.session.is_some())
        .map(|e| &e.path)
        .collect();
    let mut durations: Vec<u64> = session_paths
        .iter()
        .map(|p| active.get(*p).copied().unwrap_or(0))
        .collect();
    durations.sort_unstable();
    out.duration = DurationStats {
        median_seconds: percentile(&durations, 0.5),
        p90_seconds: percentile(&durations, 0.9),
        max_seconds: durations.last().copied().unwrap_or(0),
    };

    out
}

/// The dashboard's aggregates over every indexed transcript.
///
/// Reads the session index, which is mtime-cached, so a warm call does no
/// parsing. Deliberately NOT driven from the 3-second session poll: the rollup
/// is only worth its cost when something is going to render it.
#[tauri::command]
pub fn dashboard_stats() -> Result<DashboardStats, String> {
    let entries = crate::index::index_entries(&crate::index::projects_root());
    let observed: Vec<String> = entries
        .iter()
        .filter_map(|e| e.session.as_ref())
        .filter_map(|s| s.version.clone())
        .collect();
    let baseline = crate::version::baseline(crate::version::installed(), &observed);
    Ok(rollup(&entries, baseline.as_deref()))
}

/// Nearest-rank percentile over an ALREADY SORTED slice.
fn percentile(sorted: &[u64], q: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * q).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Whether this path is a subagent transcript rather than a session's own.
pub fn is_subagent_transcript(path: &Path) -> bool {
    parent_transcript(path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Annotation;

    fn session(id: &str, label: &str, version: &str, liveness: Liveness) -> Session {
        Session {
            session_id: id.to_string(),
            ai_title: None,
            last_prompt: None,
            git_branch: None,
            cwd: format!("/code/{label}"),
            project_label: label.to_string(),
            version: Some(version.to_string()),
            last_activity: 0,
            liveness,
            annotation: Annotation::default(),
        }
    }

    fn stats_with(day: (i32, u32, u32), tokens: (u64, u64, u64), active: u64) -> TranscriptStats {
        let mut daily = BTreeMap::new();
        daily.insert(
            NaiveDate::from_ymd_opt(day.0, day.1, day.2).unwrap(),
            DayStats {
                input_tokens: tokens.0,
                output_tokens: tokens.1,
                cache_read_tokens: tokens.2,
                tool_calls: 1,
                turns: 1,
            },
        );
        let mut tools = BTreeMap::new();
        tools.insert("Bash".to_string(), 1);
        TranscriptStats {
            first_ts: None,
            last_ts: None,
            active_seconds: active,
            turns: 1,
            daily,
            tools,
            models: BTreeMap::new(),
        }
    }

    #[test]
    fn subagent_activity_counts_toward_tokens_but_never_toward_the_session_count() {
        // The whole point of reading subagent transcripts: their work must show
        // up, without inventing sessions that never existed. Measured on a real
        // tree, they hold 16.4% of tool calls and 5.3% of tokens.
        //
        // Every token field holds a different value so a field swap cannot pass.
        let entries = vec![
            TranscriptEntry {
                path: PathBuf::from("/p/proj/abc.jsonl"),
                session: Some(session("abc", "proj", "2.1.220", Liveness::Idle)),
                stats: stats_with((2026, 8, 8), (5, 7, 9), 100),
            },
            TranscriptEntry {
                path: PathBuf::from("/p/proj/abc/subagents/agent-1.jsonl"),
                session: None,
                stats: stats_with((2026, 8, 8), (11, 13, 17), 50),
            },
        ];

        let d = rollup(&entries, Some("2.1.220"));

        assert_eq!(d.sessions_total, 1, "a subagent is not a session");
        assert_eq!(d.tokens.input, 16, "5 + 11");
        assert_eq!(d.tokens.output, 20, "7 + 13");
        assert_eq!(d.tokens.cache_read, 26, "9 + 17");
        assert_eq!(
            d.duration.max_seconds, 150,
            "the session's own 100s plus its subagent's 50s"
        );
    }

    #[test]
    fn an_outdated_session_is_counted_against_the_baseline() {
        let entries = vec![
            TranscriptEntry {
                path: PathBuf::from("/p/proj/old.jsonl"),
                session: Some(session("old", "proj", "2.1.100", Liveness::Idle)),
                stats: stats_with((2026, 8, 8), (1, 2, 3), 10),
            },
            TranscriptEntry {
                path: PathBuf::from("/p/proj/new.jsonl"),
                session: Some(session("new", "proj", "2.1.220", Liveness::Legacy)),
                stats: stats_with((2026, 8, 8), (1, 2, 3), 10),
            },
        ];

        let d = rollup(&entries, Some("2.1.220"));

        assert_eq!(d.outdated_versions, 1, "only the 2.1.100 session is behind");
        assert_eq!(d.liveness.live, 1, "Legacy counts as live");
        assert_eq!(d.liveness.idle, 1);
        assert_eq!(
            d.sessions_by_repo,
            vec![RepoStat {
                label: "proj".into(),
                sessions: 2
            }]
        );
    }

    #[test]
    fn a_subagent_transcript_attributes_to_the_transcript_that_spawned_it() {
        // Measured on a real tree: subagent transcripts hold 16.4% of all tool
        // calls. Crediting them to nobody loses that; crediting them to a session
        // of their own would invent sessions that never existed.
        let got = parent_transcript(Path::new(
            "/p/-Users-x-code/abc-123/subagents/agent-def-456.jsonl",
        ));
        assert_eq!(
            got,
            Some(PathBuf::from("/p/-Users-x-code/abc-123.jsonl")),
            "a subagent file belongs to the transcript named by its grandparent directory"
        );
    }

    #[test]
    fn the_daily_series_emits_zeros_for_days_with_no_activity() {
        // The real tree has activity on 07-28..08-02 and then nothing at all on
        // 08-03, 08-05, 08-06, 08-07. Plotting only the days present would draw
        // those four days as a single step and read as continuous work.
        let mut merged = BTreeMap::new();
        merged.insert(
            NaiveDate::from_ymd_opt(2026, 8, 8).unwrap(),
            DayStats {
                input_tokens: 5,
                output_tokens: 7,
                cache_read_tokens: 9,
                tool_calls: 2,
                turns: 3,
            },
        );
        merged.insert(
            NaiveDate::from_ymd_opt(2026, 8, 10).unwrap(),
            DayStats {
                input_tokens: 11,
                output_tokens: 13,
                cache_read_tokens: 17,
                tool_calls: 4,
                turns: 6,
            },
        );

        let series = daily_series(&merged);

        assert_eq!(
            series.iter().map(|p| p.date).collect::<Vec<_>>(),
            vec![
                NaiveDate::from_ymd_opt(2026, 8, 8).unwrap(),
                NaiveDate::from_ymd_opt(2026, 8, 9).unwrap(),
                NaiveDate::from_ymd_opt(2026, 8, 10).unwrap(),
            ],
            "the empty day between must appear"
        );
        assert_eq!(series[1].input_tokens, 0);
        assert_eq!(series[1].tool_calls, 0);
        assert_eq!(series[2].cache_read_tokens, 17);
    }

    #[test]
    fn an_ordinary_transcript_has_no_parent() {
        assert_eq!(
            parent_transcript(Path::new("/p/-Users-x-code/abc-123.jsonl")),
            None,
            "a top-level transcript owns its own activity"
        );
    }
}
