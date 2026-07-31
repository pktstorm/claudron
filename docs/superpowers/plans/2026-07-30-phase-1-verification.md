# Claudron Phase 1 — Success Criteria Verification

**Date:** 2026-07-30
**Branch:** `phase-1-implementation`
**Test totals at time of writing:** 53 Rust (+2 ignored) · 33 frontend · `clippy -D warnings` clean · `tsc --noEmit` exit 0

Criteria are from the spec. Anything marked **needs GUI** could not be measured headlessly
and is listed under "Left for the user" with exact steps.

## Results

| # | Criterion | Target | Measured | Verdict |
|---|---|---|---|---|
| 1 | Live process appears after launch | < 15 s | needs GUI | **PENDING** |
| 2 | Recover a worktree session | < 10 s | needs GUI (backend half verified) | **PENDING** |
| 3 | Notes/status survive death + restart | must persist | store round-trips, atomic write verified | **PASS (backend)** |
| 4 | Memory with 37 sessions indexed | < 500 MB | **198 MB**, flat over 5 samples | **PASS** |
| 5 | Warm poll completes | < 2 s | **19 ms** | **PASS** |

## Build and launch (verified)

- `yarn build` — succeeds. The 16 kB CSS bundle confirms Tailwind is compiling, not silently
  absent (it was missing from the project entirely until Task 12 caught it).
- `cargo build --release` — succeeds in 41.8 s, no warnings.
- The release binary launches, stays running, and logs no errors.

## Criterion 4 — memory (PASS)

Ran the release binary and sampled total RSS across all Claudron processes every 5 s for 25 s,
covering more than two 10 s poll cycles:

| t | Total RSS |
|---|---|
| 5 s | 199 MB |
| 10 s | 199 MB |
| 15 s | 198 MB |
| 20 s | 198 MB |
| 25 s | 198 MB |

Flat across cycles — the mtime cache is not accumulating, and repeated polls do not grow the
heap. 198 MB against a 500 MB bar.

Note this is with all 37 sessions indexed, exceeding the criterion's "15 sessions" wording. The
"3 terminals visible" clause does not apply — Phase 1 has no embedded terminals.

## Criterion 5 — warm poll (PASS, by a wide margin)

Measured via `cargo test index::tests::warm_scan -- --ignored --nocapture` across four runs:

| Run | Cold scan | Warm scan |
|---|---|---|
| 1 | 10.73 s | 22.1 ms |
| 2 | 10.23 s | 18.3 ms |
| 3 | 10.51 s | 18.6 ms |
| 4 | 10.18 s | 19.1 ms |

Sessions indexed: **37**, stable across all runs.

The cold figure is why the poll interval is 10 s rather than the originally planned 3 s: before
the mtime cache existed, every poll was a full 10 s rescan and polls would have overlapped
continuously. Warm scans are ~540× faster than cold.

## Criterion 3 — annotation durability (PASS on the backend)

`cargo test annotations::` — 5 passed. Covers round-trip persistence, missing-file and
corrupt-file degradation, parent-directory creation, and no-temp-file-left-behind.

Review separately traced all three crash windows (after `create_dir_all` / mid-write /
after write, before rename) and confirmed the previously-saved good file is never opened
for writing. Worst case is a stray `.tmp`, never a damaged store.

The end-to-end half — edit a note in the UI, quit, relaunch — **needs GUI**.

## Criterion 2 — the worktree `--resume` gap (backend verified, UX needs GUI)

The structural claim holds: `claude --resume` scopes to the current directory's project
directory, so a session started in a worktree is not offered when `--resume` is run from the
parent repo. Claudron indexes all 273 project directories at once, so it lists both.

**However** — measured at verification time, this is currently a latent capability rather than
an active fix. All **282** transcripts whose `cwd` is a worktree are `sdk-py` subagent runs.
**Zero** of the 37 indexed interactive sessions live in a worktree. The `--resume` gap is real
and Claudron closes it, but on today's data there is no interactive worktree session to
demonstrate it against.

What Claudron *does* fix today, on real data: `--resume` shows only the current directory's
sessions, while Claudron shows all 37 across every project, with titles, notes, and status.

## The number that matters most

**1303 transcript files on disk → 37 sessions listed.**

The `entrypoint: cli` filter removes 1266 `sdk-py` subagent transcripts. Without it the list
would be ~97% noise and the app would fail at its core purpose. This is the single most
load-bearing rule in the codebase.

Distribution of the 37 across repos:

| Sessions | Repo |
|---|---|
| 9 | `api-service` |
| 7 | `workspace` |
| 4 | `web-ui` |
| 3 | `enc-ui-aws` |
| 2 | `workspace-nonsl-tf` |
| 1 each | `s2lib`, `b4ckdr0p`, `toolkit`, … |

## Left for the user (GUI required)

Run `make dev`, then:

**Criterion 1 — a new session appears within 15 s.**
With Claudron running, open a terminal, `cd` to any repo, run `claude`, and send one message so a
transcript is written. Time until the session appears in Claudron. Expected under 15 s (one 10 s
poll plus scan).

**Criterion 2 — recovery under 10 s.**
Pick any listed session that is not running. Click "Resume in new tab". Confirm it opens an
iTerm2 tab in the right directory and resumes the right session. Time it from spotting the
session to a live prompt.

**Criterion 3 — annotations survive a restart.**
Select a session, type a note, set a status. Quit Claudron entirely. Relaunch. Confirm both
persist. Then check the store on disk: `cat ~/.claudron/annotations.json`.

**Sanity checks worth doing while the app is open:**

1. The list shows roughly **37** sessions, not 1300. A count in the hundreds means the
   `entrypoint` filter has regressed.
2. Sessions group under their repo (`api-service`, `web-ui`, …) with a readable header.
3. Typing in the search box narrows the list.
4. A session with a live `claude` process shows the **Running** badge and offers
   "Jump to terminal"; a dead one offers only "Resume in new tab".
5. Status labels read "Needs review" and "Waiting on me" — **not** `needsReview` / `waitingOnMe`.
6. If iTerm2 is closed, clicking an action shows an amber error rather than silently doing
   nothing.
