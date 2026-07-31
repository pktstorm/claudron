# Claudron — Sidebar Polish

**Date:** 2026-07-31
**Status:** Approved, pending implementation plan
**Scope:** Small. Three independent changes to existing components; no new subsystem.

## Summary

Three fixes to the session list and detail pane, arising from first real use of the app:

1. Sessions tagged **Active here** sort to the top of the sidebar.
2. Each row shows the **Claude Code version** that wrote it, highlighted when behind.
3. The detail pane's status blocks say plainly that they are **user-set**, not detected.

A fourth request — a git tab with branch cleanliness, origin sync, PR status, and worktree
cleanup — is deliberately **out of scope** and gets its own spec. It needs a new subsystem
(shelling out to `git` and `gh`, caching, async refresh) and contains the app's first
destructive action.

## Measured context

Taken from the live machine on 2026-07-31:

| Metric | Value |
|---|---|
| Interactive sessions indexed | 37 |
| Distinct working directories among them | 10 |
| **Distinct Claude Code versions across those sessions** | **16** |
| Sessions on the newest version (2.1.220) | 12 |
| Oldest version still present | 2.1.195 |

The version spread is why item 2 is worth building: more than half of all sessions were
written by something other than the current binary.

## 1. Active-here sessions sort first

`index_sessions` currently sorts by `last_activity` descending and nothing else
(`index.rs:118`). Liveness plays no part, so a session running right now can sit below one
last touched three weeks ago.

**Change:** sort by liveness rank first, then `last_activity` descending within each rank.

| Rank | Liveness | Meaning |
|---|---|---|
| 0 | `Legacy`, `Managed` | A live `claude` process is in this directory |
| 1 | `Interrupted` | Died mid-work |
| 2 | `Idle` | Ended cleanly |

`Legacy` and `Managed` share rank 0: both mean "running", and the distinction is *how* it is
hosted, which is not a reason to order one above the other.

**Filtering needs no change.** The frontend preserves backend order and `applyFilters` only
removes entries, so a filtered list keeps the same relative ordering. This satisfies the
requirement that active sessions lead both unfiltered and filtered results.

**A caveat carried from Phase 1, unchanged by this work.** "Active here" means a live
`claude` process shares the session's *directory* — not that this specific session owns it.
Two sessions in one repo both show the tag, so both sort to the top. Sorting inherits that
imprecision rather than introducing it. Making liveness per-session requires the tmux work
in Phase 2B.

## 2. Claude Code version on each row

`Session.version` is already parsed, typed, and sent to the client. It is rendered only in
the detail pane footer, where it does not help you scan.

**Display:** each row shows the version (`v2.1.205`) in muted grey, beside the existing
relative-age text. A session whose transcript carries no version renders nothing — no
placeholder.

The row header now holds four items: title, version, age, liveness badge. The title is the
only one that may truncate; version, age, and badge are all `shrink-0`. The sidebar is 320 px
wide and the three fixed items total roughly 130 px, leaving the title the remainder — enough
for a readable prefix at the 900 px minimum window width.

**Highlighting:** a version below the baseline renders amber instead of grey. Both the value
and the highlight are always present, so an exact version is available at a glance and stale
ones still stand out.

### The baseline

```
baseline = max(installed binary version, highest version seen while indexing)
```

Checking only the installed binary is not sufficient. Claude Code **auto-updates**, so the
binary can change while Claudron is running; a value cached at startup would mark
freshly-upgraded sessions as behind the stale cache — exactly backwards.

Taking the running max fixes this without extra machinery: when a session appears carrying a
newer version, the session data itself proves a newer version exists and the baseline moves.
No re-invocation, no polling, no staleness window.

The binary check still earns its place for one case: a fresh upgrade where every existing
session predates it. Then no session evidences the new version and only the binary knows.
Run it once at startup; if it fails, fall back to the highest observed version rather than
disabling the feature.

### Version comparison

Compare numerically, segment by segment on `.`, **not** as strings. String comparison places
`2.1.99` above `2.1.220`, which would mark the newest sessions as outdated and the oldest as
current. A segment that does not parse as a number compares as 0 rather than panicking.

### What the marker means

A session's version is whatever wrote its transcript. For a live session that is what is
running now. For an idle one it reflects when it last ran — resuming it would pick up the
current binary. The marker therefore reads as "written by an older version", which is what
matters for live sessions and is honest age information for dead ones.

## 3. Status blocks are user-set

The detail pane has a `Status` heading above four clickable blocks (`blocked`,
`needs-review`, `waiting-on-me`, `background`). Nothing indicates they are inputs, and
they sit near the automatic liveness badge, so they read as a readout.

**Change:** the heading becomes `Your status`, with one line under it: *"Set a label for
yourself — this is not detected."*

No behavioural change: the picker already toggles and persists correctly.

## Out of scope

- **The git tab** — branch cleanliness, origin sync, worktrees used, PR and CI status, and
  worktree cleanup. Its own spec. `gh` is installed and authenticated and the repos have
  remotes, so it is feasible; it is simply a much larger piece of work with a destructive
  action in it.
- **Showing the working directory in the sidebar.** Group headers are already derived from
  `cwd` via `project_label`, so a per-row directory would duplicate them for most sessions.
  The full path remains in the detail pane footer. Revisit only if the version badge lands
  and the gap still feels real.

## Testing

- **Sort** — unit tests over a fixture set proving live sessions precede interrupted and
  idle ones regardless of activity time, that ordering within a rank stays
  most-recent-first, and that `Legacy` and `Managed` tie.
- **Version comparison** — tests covering the string-sort trap (`2.1.99` vs `2.1.220`),
  equal versions, missing versions, and unparseable segments.
- **Baseline** — tests proving the running max wins when a session carries a version newer
  than the binary, and that a failed binary check falls back to the highest observed.
- **Rendering** — component tests asserting the version renders, that a below-baseline
  version is styled differently from a current one, and that a session without a version
  renders no badge.

## Success criteria

1. With at least one live session, that session appears at the top of the sidebar without
   any filter applied.
2. Applying a filter that includes live sessions still lists them first.
3. Every row whose transcript carries a version displays it.
4. A session on a version below the baseline is visually distinct from one on the baseline.
5. `2.1.99` is correctly treated as older than `2.1.220`.
6. The detail pane states that its status blocks are user-set.
