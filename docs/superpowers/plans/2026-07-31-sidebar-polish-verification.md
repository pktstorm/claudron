# Claudron Sidebar Polish — Success Criteria Verification

**Date:** 2026-07-31
**Branch:** `sidebar-polish`
**Test totals at time of writing:** 106 Rust passed (+7 ignored) · 78 frontend passed across 11
files · `clippy -- -D warnings` clean · `tsc --noEmit` exit 0 · `yarn build` succeeds

## Context

This pass was motivated by a version spread of **16 distinct `claude` versions across 37
indexed sessions, with only 12 sessions on the newest version** — most sessions in the sidebar
were running something other than current. The sidebar previously gave no way to see this at a
glance.

The installed `claude` binary on this machine reports **2.1.220**
(`cargo test version::tests::reads_the_real -- --ignored --nocapture`). This is one half of the
baseline computed by `version::baseline` in `src-tauri/src/version.rs`: the baseline is
`max(installed version, every version observed in a session transcript)`, so a session carrying a
newer version than the binary read at startup (Claude Code auto-updates while Claudron runs) can
still raise the baseline. 2.1.220 is the value used throughout this record wherever a concrete
baseline is needed.

Criteria 1, 2, 3, 4, and 6 have unit-test coverage for their underlying logic but their end-to-end
truth — what actually renders in the running app — requires a GUI and could not be measured
headlessly; they are marked **NEEDS-GUI** below with exact steps for a human to follow, in the
same format as `docs/superpowers/plans/2026-07-31-phase-2a-verification.md`. Criterion 5 is
covered end-to-end by automated tests in both languages and is marked **PASS**.

## Results

| # | Criterion | Automated coverage | Verdict |
|---|---|---|---|
| 1 | A live session appears at the top of the sidebar, unfiltered | `live_sessions_sort_above_idle_ones_however_old`, `interrupted_sorts_between_live_and_idle` (`src-tauri/src/index.rs`) | **NEEDS-GUI** |
| 2 | A filter that includes live sessions still lists them first | `applyFilters` tests in `src/store/filters.test.ts` (order-preserving `Array.filter`) + criterion 1's sort tests | **NEEDS-GUI** |
| 3 | Every row whose transcript carries a version displays it | `shows the session's Claude version`, `renders no version badge when the session has none` (`src/components/SessionList.test.tsx`) | **NEEDS-GUI** |
| 4 | A below-baseline version is visually distinct from a baseline one | `styles an outdated version differently from a current one`, `does not mark anything stale when there is no baseline` (`src/components/SessionList.test.tsx`) | **NEEDS-GUI** |
| 5 | `2.1.99` is treated as older than `2.1.220` | `compares_numerically_not_as_strings` (`src-tauri/src/version.rs`) and `"compares numerically, not as strings"` (`src/version.test.ts`) | **PASS** |
| 6 | The detail pane states its status blocks are user-set | `says the status blocks are user-set, not detected` (`src/components/SessionDetail.test.tsx`) | **NEEDS-GUI** |

## Criterion 5 — `2.1.99` is older than `2.1.220` (PASS)

Covered by direct unit tests in both languages, each asserting the numeric (not lexical) ordering
by name:

```
cd src-tauri && cargo test version::tests::compares_numerically_not_as_strings
```

```rust
#[test]
fn compares_numerically_not_as_strings() {
    // The whole feature inverts if this is a string comparison:
    // "2.1.99" > "2.1.220" lexically, but 99 < 220 numerically.
    assert!(is_older("2.1.99", "2.1.220"));
    assert!(!is_older("2.1.220", "2.1.99"));
}
```

```
yarn vitest run src/version.test.ts
```

```ts
it("compares numerically, not as strings", () => {
  expect(isOlder("2.1.99", "2.1.220")).toBe(true);
  expect(isOlder("2.1.220", "2.1.99")).toBe(false);
});
```

Both tests passed as part of the full suite runs recorded below (106 Rust passed, 78 frontend
passed). This criterion is the one case in this pass where the comparison itself — not just its
use inside a UI — is the whole of what's being asked, so unit coverage in both languages is
sufficient; no GUI step is needed.

## Left for the user (GUI required)

Run `make dev`, then:

**Criterion 1 — a live session sits at the top of the sidebar with no filter applied.**
With Claudron open and no filter selected, start (or find) a session with an actively running
`claude` process (`cd` into any indexed repo and run `claude`, or use one already running).
Confirm the sidebar re-indexes it within a poll cycle and that it appears above every session
badged "Idle" — regardless of how recently those idle sessions were touched. `sort_sessions` in
`src-tauri/src/index.rs` ranks liveness first and recency only as a tiebreaker within a rank, so
the concrete thing to look for is: sessions badged "Active here" / "Managed" / "Interrupted" all
sit above every "Idle" session, in that rank order, even if an idle session's last-activity
timestamp is more recent.

**Criterion 2 — a filter that includes live sessions still lists them first.**
With the same live session from criterion 1 still running, apply a filter that keeps it in the
result set — e.g. filter by its project's liveness badge, or type a search term that matches its
title, branch, or notes. Confirm it still renders above the filtered list's idle sessions. This is
expected to hold automatically: `applyFilters` in `src/store/filters.ts` is a plain
`Array.prototype.filter`, which only removes elements and never reorders the array the backend
already sorted — so nothing at the filtering layer can undo criterion 1's ordering. Confirm this
holds visually rather than assuming it from the code.

**Criterion 3 — every row whose transcript carries a version displays it.**
With Claudron open, scan the sidebar for any session whose transcript records a `claude` version
(this will be most sessions, given the 37-session / 16-version spread that motivated this work).
Confirm each such row shows a small `vX.Y.Z` badge next to its liveness badge, and that sessions
with no recorded version (older transcripts, or ones from before version-tagging existed) show no
badge at all rather than a blank or placeholder one.

**Criterion 4 — a below-baseline version is visually distinct from one on the baseline.**
Find (or produce, by filtering/searching) at least one session on a version older than 2.1.220 and
one on 2.1.220 itself. Confirm the older one's version badge renders in a distinct color (amber,
per `SessionRow.tsx`'s `text-amber-400` vs `text-neutral-600`) from the current one's, so an
outdated session is visually flagged without reading the exact number. With 16 versions across 37
sessions and only 12 on the newest, most rows in the real sidebar should show the amber (outdated)
styling — if instead most rows look identical, that is a sign the baseline or the comparison isn't
wired up correctly in the running app.

**Criterion 6 — the detail pane states its status blocks are user-set.**
Select any session in the sidebar to open its detail pane. Confirm the "Your status" section
displays the sentence "Set a label for yourself — this is not detected." (or equivalent copy
conveying the same fact) directly under the section heading, so a user reading the pane
understands the status is something they set themselves, not something Claudron inferred from the
transcript.

## Full suite at time of writing

- `cd src-tauri && cargo test` — **106 passed, 0 failed, 7 ignored**.
- `cd src-tauri && cargo clippy -- -D warnings` — clean.
- `yarn vitest run` — **78 passed** across **11 files**.
- `yarn tsc --noEmit` — clean, exit 0.
- `yarn build` — succeeds.
- `cd src-tauri && cargo test version::tests::reads_the_real -- --ignored --nocapture` — installed
  `claude` binary reports **2.1.220**.
