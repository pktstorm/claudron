# Claudron Phase 2A — Success Criteria Verification

**Date:** 2026-07-31
**Branch:** `phase-2a-conversation-view`
**Test totals at time of writing:** 91 Rust passed (+6 ignored) · 66 frontend passed ·
`clippy -- -D warnings` clean · `tsc --noEmit` exit 0 · `yarn build` succeeds

Criteria are from the Phase 2A spec. Criteria 3 and 4 require a running GUI and could not be
measured headlessly; they are marked **NEEDS-GUI** below with exact steps for a human to follow,
in the same format as `docs/superpowers/plans/2026-07-30-phase-1-verification.md`.

## Results

| # | Criterion | Target | Measured | Verdict |
|---|---|---|---|---|
| 1 | Largest real transcript renders (release build) | < 1 s | **148.638542 ms** (72,049,886 bytes → 9,335 turns) | **PASS** |
| 2 | Warm poll of an unchanged conversation | < 50 ms | **48.125 µs** | **PASS** |
| 3 | A new turn appears in an open conversation | < 2 s of reaching disk | not measured headlessly | **NEEDS-GUI** |
| 4 | Memory with the largest conversation open | < 500 MB | not measured headlessly | **NEEDS-GUI** |
| 5 | Every real record type is rendered or explicitly ignored | no silent drops | 16 distinct types seen across the full 1,368-file corpus, all classified | **PASS** |

## Criterion 1 — largest transcript parses in under 1 s (PASS)

```
cd src-tauri && cargo test --release conversation::parse::tests::parses_the_largest -- --ignored --nocapture
```

```
parsed 72049886 bytes -> 9335 turns in 148.638542ms
```

148.6 ms against a 1 s budget in the release build — roughly 6.7x margin. (The `debug_assertions`
budget in this same test is deliberately loosened to 3 s so the test stays runnable, and useful,
during ordinary development; the criterion itself is about the shipped release binary.)

## Criterion 2 — warm poll under 50 ms (PASS, by a wide margin)

Added `a_warm_poll_of_the_largest_real_transcript_is_fast` to
`src-tauri/src/conversation/tail.rs`'s `tests` module (it did not exist before this task). It
finds the largest real depth-2 transcript, does a cold `read_from` to reach EOF, then times a
second `read_from` at the same offset with the carried `pending` state — the exact call the 1 s
polling loop makes when nothing has changed.

```
cd src-tauri && cargo test --release conversation::tail::tests::a_warm_poll -- --ignored --nocapture
```

```
file 72049886 bytes, cold turns 9335, warm turns 0, warm poll 48.125µs
```

48.125 µs against a 50 ms budget — about 1,000x faster than the bar. The warm path costs almost
nothing: a `stat`, a re-open, a seek to the last offset, and hitting EOF immediately — no
re-parsing of the 72 MB already consumed.

## Criterion 5 — no record type is silently dropped (PASS)

Added `every_real_record_type_is_handled_or_deliberately_ignored` to
`src-tauri/src/conversation/parse.rs`'s `tests` module (it was scoped to Task 10 rather than
Task 2, so it did not exist before). It walks every real `.jsonl` transcript under
`~/.claude/projects` (or `$CLAUDRON_PROJECTS_DIR` if set), collects every distinct `type` field
seen, and fails if any value is not in a hardcoded `KNOWN` allowlist.

**This test was first written with a 60-file cap** (matching the brief verbatim) and passed
immediately, seeing only 6 distinct types. That pass was a false negative: capping the scan to
~4% of the corpus meant the one test whose entire job is catching unknown record types was
not actually exercising most of the corpus. Scanning the full 1,368-file corpus surfaced three
types the capped scan never reached:

| Type | Occurrences | Shape | Classification |
|---|---|---|---|
| `agent-name` | 1,992 | `{ agentName, sessionId }` | control-plane, ignored |
| `relocated` | 1,540 | `{ sessionId, relocatedCwd }` | control-plane, ignored |
| `worktree-state` | 1,539 | `{ worktreeSession: {...} }` | control-plane, ignored |

None of the three carries a `message` field, so none is renderable conversation — the parser's
existing rule (only `assistant` and `user` become turns; everything else is control-plane) was
already correct. This was a **test defect** (an under-sampling cap producing a false PASS), not a
parser defect; the parser was not changed. The cap was removed and all three types were added to
`KNOWN` with a comment recording the render-vs-ignore decision.

```
cd src-tauri && cargo test conversation::parse::tests::every_real_record -- --ignored --nocapture
```

```
record types seen across 1368 files: {"agent-name", "ai-title", "assistant", "attachment", "file-history-delta", "file-history-snapshot", "frame-link", "last-prompt", "mode", "permission-mode", "pr-link", "queue-operation", "relocated", "system", "user", "worktree-state"}
test conversation::parse::tests::every_real_record_type_is_handled_or_deliberately_ignored ... ok
```

All 16 types in `KNOWN` were observed across the full corpus and the test passes. Runtime for the
full scan is ~10.3s (measured directly in the test's own timing, ~13.8s wall including
compilation) — an acceptable cost for an `#[ignore]`d test run on demand, not part of the default
`cargo test` suite.

**Sanity check that the test actually discriminates:** `"agent-name"` was temporarily removed from
`KNOWN` and the test re-run. It failed exactly as expected:

```
thread 'conversation::parse::tests::every_real_record_type_is_handled_or_deliberately_ignored' panicked at src/conversation/parse.rs:484:9:
unrecognised record type(s) ["agent-name"] -- decide whether they render or are ignored, then add them to KNOWN
test conversation::parse::tests::every_real_record_type_is_handled_or_deliberately_ignored ... FAILED
```

`agent-name` was then restored to `KNOWN` and the test passes again. This is direct evidence the
full-corpus version of the test discriminates correctly — unlike the capped version, which passed
for the wrong reason (under-sampling, not correctness).

This test is exactly the safety net criterion 5 asks for: if Claude Code ever introduces a new
record `type`, this test fails the next time someone runs the ignored suite against real data,
rather than the new type silently vanishing from the conversation view. It is intentionally
`#[ignore]`d (it depends on the local machine's real transcript corpus) and intentionally *not*
self-healing — a failure here should prompt a human decision (render vs. ignore), never a reflex
addition to `KNOWN`.

## Left for the user (GUI required)

Run `make dev`, then:

**Criterion 3 — a new turn appears within 2 s of reaching disk.**
Open Claudron, select a conversation for a session with an actively running `claude` process (or
start one: `cd` into any indexed repo and run `claude`). From that session's terminal, send a
message and wait for a response. With the Claudron conversation view open on that session, time
from when the terminal shows the new content to when the same turn appears in Claudron. Expected
under 2 s — the conversation view polls at 1 s, so worst case is just under two poll cycles.

**Criterion 4 — memory stays under 500 MB with the largest conversation open.**
With Claudron running, open the largest transcript from criterion 1 (72,049,886 bytes / 9,335
turns) in the conversation view. Let it sit for at least two poll cycles so any per-poll
allocation has a chance to show up. Sample total RSS across all Claudron processes:

```
ps -eo pid,rss,comm | grep -i claudron
```

Sum the RSS column (in KB) across all matching processes, convert to MB, and sample at least
3 times over ~15 s to confirm it is flat rather than climbing. Compare against the 500 MB bar
established in Phase 1 (that measurement was 198 MB with 37 sessions indexed but no conversation
open; this criterion adds the cost of holding 9,335 parsed turns in memory in the frontend and
Rust layers simultaneously).

**Sanity checks worth doing while the app is open:**

1. Opening the largest transcript does not visibly freeze the UI thread — parsing 72 MB in
   ~150 ms should be imperceptible, but confirm the conversation pane doesn't show a blank/frozen
   state for longer than a beat.
2. Scrolling the largest conversation is smooth, not janky, once loaded.
3. A tool call with no result yet (an in-flight `claude` session) renders with no result rather
   than blanking the turn.
4. Switching away from a conversation and back does not re-trigger a full re-parse from byte 0 —
   confirm via the warm-poll behaviour above that the offset is honored.

## Full suite at time of writing

- `cd src-tauri && cargo test` — **91 passed, 0 failed, 6 ignored** (4 ignored at HEAD before this
  task + the 2 new ignored tests added in this task: `every_real_record_type_is_handled_or_deliberately_ignored`
  and `a_warm_poll_of_the_largest_real_transcript_is_fast`).
- `cd src-tauri && cargo clippy -- -D warnings` — clean.
- `yarn vitest run` — 66 passed (unchanged from HEAD; this task touched no frontend code).
- `yarn tsc --noEmit` — clean, exit 0.
- `yarn build` — succeeds (unchanged from HEAD).
