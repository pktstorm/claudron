# Claudron — Per-Transcript Stats Extraction

**Date:** 2026-08-08
**Status:** Implemented. Cost section carries measured figures, not predictions.
**Scope:** Medium. One existing Rust module restructured, one new Rust module, one new dependency. No UI.
**Issues:** [#15](https://github.com/pktstorm/claudron/issues/15) (stats dashboard) — the extraction half
**Related:** [#13](https://github.com/pktstorm/claudron/issues/13) (shadcn), [#42](https://github.com/pktstorm/claudron/issues/42) (SQLite), [#53](https://github.com/pktstorm/claudron/issues/53) (cold scan)

## Summary

The stats dashboard (#15) asks for token totals, cache hit rate, tool usage, session duration, and
three time-series charts. **None of that data exists anywhere in Claudron today.** The session index
caches a `Session` — identity, cwd, liveness, version, last activity — and nothing about what a
session did. `Usage` is parsed per turn, but only when a single conversation is opened on demand.

This change extracts per-transcript statistics during the walk that already happens, and rolls them
up behind one command. It ships no UI.

## Why this is a separate piece of work

Planning #13 and #15 together surfaced four separable pieces, not two:

| | Piece | Depends on | Character |
|---|---|---|---|
| **A** | **Per-transcript extraction — this document** | nothing | Rust only |
| B | shadcn foundation (#13) | nothing | Frontend only; forces a theming decision |
| C | Top-level navigation | nothing | `App.tsx` has no router; no issue owns this |
| D | Dashboard UI (#15) | A, B, C | Cards and charts |

A and B are genuinely parallel: different languages, no shared files. C is unfiled — `App.tsx:113-146`
renders one fixed layout with no view switching, so a dashboard has nowhere to live yet.

### #15's stated cost mitigation does not work

#15 says: *"Reuse the existing index cache rather than re-walking, and compute aggregates
incrementally."* The existing cache is:

```rust
type CacheEntry = (Freshness, Option<Session>);          // index.rs:32
```

`Session` (`model.rs:34-45`) carries `session_id`, `ai_title`, `last_prompt`, `git_branch`, `cwd`,
`project_label`, `version`, `last_activity`, `liveness`, `annotation`. There are no tokens, no tool
counts, no start time, and no turn count. The instruction cannot be followed as written, because the
cache does not hold the values. Closing that gap is what this document specifies.

Of #15's requested statistics, roughly half are already free from the index — total sessions, live
count, liveness breakdown, sessions per repository, outdated versions, interrupted rate. Every
token statistic, every tool statistic, every duration, and all three charts require this work.

## Measured context

Taken on 2026-08-08 across every transcript under `~/.claude/projects` on this machine.

**Scale caveat, stated plainly:** this machine holds **31 transcript files**. The "1461 files /
11.6 s cold scan" figure in #53 is from a different machine. Nothing below establishes how this
behaves at 47× the file count, and the implementation must re-measure rather than extrapolate.

| | |
|---|---|
| Files · lines · bytes | 31 · 7,775 · 77.3 MB |
| Records carrying `usage` | 3,262 |
| Tokens | input 721K · output 3.38M · cache-read **399.8M** |
| Cache hit rate | **99.8%** |
| `tool_use` blocks | 1,602 across 34 distinct tools (Bash 774, Read 254, Edit 142) |
| Distinct models | 6, including `<synthetic>` |
| Transcripts with derivable duration | 31 of 31 |

### Finding 1 — extraction rides a pass that already happens

`parse_transcript` (`transcript.rs:24-28`) reads every line of every transcript and builds a full
`serde_json::Value` for each, then discards it. Summing `usage` and counting `tool_use` are field
lookups on data already in memory: no extra I/O, no extra JSON parse.

### Finding 2 — the sidechain early return is the only real cost

`transcript.rs:32-34` returns `None` the moment it sees an `isSidechain` record. Measured, that
fires on **line 1 in all 13 sidechain-containing files**, so those files are abandoned immediately
today. Parsing them fully costs **3.2 MB of 77.3 MB — 4.1% more bytes**. That figure is the whole
marginal cost of this change on the scan path.

### Finding 3 — the index is blind to 16.4% of tool calls

| | Rejected as sidechain | Indexed |
|---|---|---|
| `tool_use` blocks | 263 | 1,341 |
| Tokens | 21.4M | 383.2M |

**16.4% of tool calls and 5.3% of tokens are invisible** to anything derived from the index.
Subagents do proportionally far more tool work than they consume tokens, which is what delegated
search and editing looks like. A "most-used tools" chart built on the index alone would read
knowably low with nothing indicating it.

Of the 13 sidechain files, 12 are subagent transcripts under a `subagents/` directory. **One is a
top-level session transcript**, which means that entire session is rejected from the index and never
appears in Claudron at all. That is pre-existing behaviour, out of scope here, and filed separately.

### Finding 4 — per-day bucketing is mandatory

| | |
|---|---|
| Median session duration | 32.5 min |
| p90 | ~49.5 hours |
| Longest | 186.6 hours |
| **Spanning more than one calendar day** | **7 of 31 — 23%** (up to 4 days) |

Attributing a session's totals to its start date would misreport 23% of sessions, some by days. The
time-series charts must be built from per-day buckets computed during extraction.

This also disambiguates #15's "median and longest session duration": the 186-hour session is one
resumed across four days, not four days of work. Wall-clock span and active time are different
questions and both are kept.

### Finding 5 — UTC bucketing is one late night from being wrong

All recorded activity falls between 13:00 and 23:00 UTC (09:00–19:00 local, EDT), so UTC and local
dates agree on **100%** of 5,772 timestamped records today. That is an accident of working hours,
not a property of the data: 23:00 UTC already has records, and anything after 20:00 local lands on
the following UTC day. Bucketing by UTC would fail silently, and only for evening work.

### Finding 6 — the day series has real gaps

Activity exists on 2026-07-28 through 08-02, then **nothing on 08-03, 08-05, 08-06, or 08-07**. An
area chart plotting only the days present compresses those gaps and overstates continuity.
Gap-filling is a requirement of the rollup, not a rendering detail.

## Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Persistence | In-memory, extending the existing cache | No new storage; keeps A independent of #42, which is unstarted |
| Subagent transcripts | Contribute stats, attributed to the parent session | Fixes the 16.4% undercount; never create a session row |
| Day buckets | Required, keyed by **local** date | 23% of sessions span days; UTC fails silently in the evening |
| Timezone handling | Add `chrono` | Correct in every zone including half-hour offsets; keeps aggregation in Rust |

Adding `chrono` is a real cost in a tree with five dependencies, and #65 is actively about
dependency bloat. It is accepted because the alternative is a chart that is wrong for evening work
with nothing on screen saying so, and because the only dependency-free alternative — hourly UTC
buckets re-folded in React — breaks on +05:30 offsets and moves aggregation into the UI, against the
architecture rule that React only renders.

`chrono` is needed with its `serde` feature so `NaiveDate` serializes as `"2026-08-08"` across the
Tauri boundary, and with local-timezone support enabled. Confirm the resulting transitive tree
against #65 before committing to it — if it drags in duplicate versions of crates already in the
graph, that is worth knowing at implementation time rather than at review.

## Architecture

`parse_transcript` currently does two jobs in one pass and conflates them: deciding *whether this is
a session*, and extracting *session fields*. This change adds a third — extracting statistics — over
a **wider** set of files than the first job accepts.

```rust
pub struct ParsedTranscript {
    /// None when this transcript is not a session: sidechain, non-cli, or no turns.
    pub summary: Option<TranscriptSummary>,
    /// Always produced. Subagent transcripts have stats but are never sessions.
    pub stats: TranscriptStats,
}

/// None only on I/O failure. A transcript that is not a session still returns
/// its stats.
pub fn parse_transcript(path: &Path) -> Option<ParsedTranscript>
```

The rejection rules keep their meaning; they stop truncating the loop. `saw_sidechain` becomes a
flag consulted at the end alongside the existing `is_cli` and `has_conversation` checks. This is the
entire +4.1%.

**Attribution is by path, not content.** `subagent::subagent_path` builds
`<stem>/subagents/agent-<id>.jsonl`, so a stats record found beneath a `subagents/` directory belongs
to the session named by the directory two levels up. No extra parsing establishes the link.

**The walk had to grow to reach them at all.** Not anticipated at design time: `index.rs` walked
with `max_depth(2)`, and a subagent transcript sits at depth 4
(`<project>/<stem>/subagents/agent-<id>.jsonl`). Every subagent file was therefore invisible to the
scan regardless of what `parse_transcript` did with it. The dashboard walk uses depth 4; the session
walk stays at depth 2 and skips subagent transcripts, for the warm-path reason in *Cost*.

**Cache** gains one slot, keyed on the same `(mtime, size)` freshness that already invalidates per
file:

```rust
type CacheEntry = (Freshness, Option<Session>, TranscriptStats);
```

**New module `src-tauri/src/stats.rs`** owns the rollup and a single command. Without it, A produces
nothing observable and cannot be tested end to end; with it, D has a stable interface to build
against while B and C proceed in parallel.

## Data model

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStats {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub tool_calls: u32,
    pub turns: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptStats {
    pub first_ts: Option<String>,
    pub last_ts: Option<String>,
    /// Sum of gaps between consecutive turns that are BELOW `IDLE_GAP`.
    pub active_seconds: u64,
    pub turns: u32,
    /// Keyed by LOCAL calendar date.
    pub daily: BTreeMap<NaiveDate, DayStats>,
    /// Full tool names, including `mcp__plugin_github_github__issue_read`.
    pub tools: BTreeMap<String, u32>,
    pub models: BTreeMap<String, u32>,
}
```

`active_seconds` exists because of the 186-hour session. `first_ts`/`last_ts` answer "when did this
start and end"; `active_seconds` answers "how long was this session". #15 asks for the second while
implying the first. `IDLE_GAP` is a named, tested constant — proposed at **5 minutes** — not a
number folded into an expression.

`BTreeMap` rather than `HashMap` throughout: the maps are small, and deterministic ordering makes
assertions stable and serialized output diffable.

### Rollup

```rust
#[tauri::command]
pub fn dashboard_stats() -> Result<DashboardStats, String>
```

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardStats {
    pub sessions_total: u32,
    pub sessions_by_liveness: BTreeMap<Liveness, u32>,
    pub sessions_by_repo: Vec<RepoStat>,      // project_label, count
    pub outdated_versions: u32,
    pub tokens: TokenTotals,                  // input, output, cache_read, hit_rate
    pub daily: Vec<DayPoint>,                 // gap-filled, local dates, ascending
    pub top_tools: Vec<ToolStat>,             // name, count, descending
    pub duration: DurationStats,              // median, p90, max over active_seconds
}
```

`daily` is a `Vec` rather than a map because it is an ordered, gap-filled series — the ordering and
the filled zeros are the contract, and a map would let a caller iterate it in a way that loses both.

Two invariants the rollup must hold, both of which the raw data will not give for free:

1. **Subagents never inflate session counts.** Counts come from `summary.is_some()`; activity comes
   from every `TranscriptStats` regardless.
2. **The daily series is gap-filled.** Every calendar date between the first and last with activity
   appears, with zeros where nothing happened.

The rollup runs over cached structs and touches no files. It must **not** be driven from the
3-second session poll — only when the dashboard asks.

## Cost

Predicted at design time: **+4.1% of bytes**, from removing the sidechain early return.

**Measured after implementation**, ten runs per side against a *frozen copy* of the transcript tree
(31 files), using the `#[ignore]`d timing harness in `index.rs`:

| | cold (median) | warm (median) |
|---|---|---|
| before | 404 ms | 426 µs |
| after | 422 ms | 438 µs |
| delta | **+4.5%** | +3%, within run-to-run noise |

The cold delta landed within half a point of the prediction. Two things had to be got right first,
and both were found by measuring rather than by reading:

- **Measure against a frozen tree.** Run against the live `~/.claude/projects` and the numbers are
  contaminated: the running session's own transcript is appended mid-run, the warm scan takes a
  genuine cache miss on it, and the result is a ~26 ms outlier in roughly 1 run in 7. That looks
  exactly like a race and is not one. The first measurement taken this way reported +12% cold.
- **`include_subagents` is what protects the warm path.** Raising the walk to depth 4 took the file
  count from 19 to 31, and the session list -- which reads no statistics at all -- paid for every
  one of them on a poll that runs continuously. The session walk therefore stays at depth 2 and
  skips subagent transcripts.

Because a session-only walk never visits subagent transcripts, `cache.retain` must not read their
absence as deletion. Evicting them there would make the 3-second session poll discard the
dashboard's cached statistics and force a full re-parse on every dashboard open.

The 20-iteration concurrency loop required for changes touching shared state passed with 0 failures.
- Memory: at 31 files the daily maps are trivial. At #53's 1461 files, with a median span of one day,
  the order is a few thousand small entries. Worth confirming, not worth pre-optimising.

## Testing

Every test below is chosen because a plausible wrong implementation fails it.

**Day bucketing**

- A fixture transcript with turns on two different local days produces **two** `daily` entries with
  the tokens split correctly. Fails any implementation attributing totals to the start date.
- A turn at 23:30 local (03:30 UTC the next day) buckets to the **local** date. Fails a
  `&ts[..10]` string-slice implementation — the specific shortcut this design rejects.

**Sidechain and attribution**

- A subagent transcript yields `summary: None` **and** non-empty stats. Fails both the retained early
  return and any implementation that lets a subagent become a session.
- Stats from `<stem>/subagents/agent-x.jsonl` roll into the session for `<stem>.jsonl`; the session
  count is unchanged by adding a subagent file. Fails attribution by content rather than path.

**Duration**

- A fixture spanning four days with short bursts of activity yields `active_seconds` near the sum of
  the bursts, not the span. Fails `last_ts - first_ts`.
- A gap exactly at `IDLE_GAP` is treated deterministically, with the boundary asserted rather than
  left to chance.

**Rollup**

- A rollup over sessions active on day 1 and day 3 emits day 2 with zeros. Fails an implementation
  that plots only present days.
- Cache hit rate is computed as `cache_read / (cache_read + input)`, asserted against a fixture whose
  three token fields hold **three different values** — per this repository's rule that fixtures must
  not let distinct fields hold equal values, since that would make field-swap bugs undetectable.
- Tool names are preserved verbatim, including a long `mcp__…` name, rather than truncated or
  normalised.

**Parsing robustness**

- A malformed line is skipped without dropping the lines after it.
- A transcript with no timestamps yields `first_ts: None` and an empty `daily`, not a panic.

## Out of scope

- All UI. No component, no chart, no navigation.
- Persistence between launches. Stats die with the process; #42 owns durable storage.
- Cost in currency. #16 owns pricing.
- Fixing the cold scan. #53 owns that; this change must simply not make it materially worse.
- The rejected top-level sidechain transcript (Finding 3). Filed separately.

## Success criteria

- `make test` and `make lint` both pass.
- `dashboard_stats()` returns token totals, tool ranking, per-day series, and duration percentiles
  over the real transcript tree.
- A subagent transcript contributes its tool calls and tokens while leaving the session count
  unchanged, proven by a test that fails if either half is wrong.
- A session spanning two local days appears on two bars.
- The cold-scan delta is **measured** in Rust and reported as a before/after pair, not estimated.
- The warm poll shows no regression against its ~19 ms baseline.

## Follow-ups to file

1. **One top-level transcript contains a sidechain record**, so that session is rejected from the
   index entirely and is invisible in Claudron. Pre-existing; unrelated to the dashboard.
2. **#15 should be corrected** — its "reuse the existing index cache" mitigation is not achievable
   against the current `CacheEntry`, and the issue will mislead whoever picks it up next.
3. **Cache hit rate is 99.8%** on real data. #15 calls it "the most actionable number for cost", but
   it is a flat line near 100%; worth deciding whether it deserves a chart or a single figure.
4. **Piece C — top-level navigation** has no issue. It blocks D and should exist before D starts.
