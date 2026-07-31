# Claudron — Design

**Date:** 2026-07-30
**Status:** Approved, pending implementation plan

## Problem

Running 10+ concurrent Claude Code sessions in iTerm2 tabs creates three distinct problems:

1. **Lost work.** Closing a tab kills the `claude` process. Recovery is a hunt, and sessions
   sometimes fail to appear in `claude --resume` at all.
2. **No visibility.** No way to see what each session is doing without switching to it, and
   nowhere to record notes about what to come back to.
3. **Unreadable tabs.** iTerm2 tab titles squish together at 10+ tabs; none is identifiable.

### Measured baseline (2026-07-30)

Taken from the live machine, not estimated:

| Metric | Value |
|---|---|
| Live `claude` processes | 11 |
| Project directories under `~/.claude/projects` | 273 |
| Transcripts modified in the last 3 days | 387 |
| Interactive (`entrypoint: cli`) among 400 most recent transcripts | **21 (5%)** |
| Programmatic (`entrypoint: sdk-py`) among the same 400 | **379 (95%)** |
| Interactive sessions flagged `interruptedByShutdown` | **0 of 21** |

### Correction to an earlier premise

An initial sample suggested 20% of sessions died mid-work. That figure counted **all** transcripts,
which are dominated by `sdk-py` subagent runs where shutdown-interruption is routine and harmless.
Filtering to interactive `cli` sessions, the interrupted rate is **0 of 21**.

Two consequences: interrupted-session recovery is a **minor** feature, not a headline one (it stays
because it is nearly free once transcripts are parsed, but it does not drive the design); and
**filtering by `entrypoint` is the single most important correctness rule in the index** — without
it the list is 95% noise.

### Root cause of the `--resume` gap

`claude --resume` is scoped to the current working directory's project directory. Many sessions
run in git worktrees, which map to their own project directories (e.g.
`-Users-sthirlwall-code-workspace-api-service--worktrees-feature-work`). A session started in a
worktree is **invisible** to `--resume` run from the main repo. This is not a bug to work around
but a scoping behavior to design past: Claudron indexes all project directories at once.

## Prior art

**`claude agents`** (official, Claude Code v2.1.139+; local version is 2.1.220) provides a single
list of running sessions with state icons and peek/attach. It does not provide persistent
per-session notes, protection against accidental closure, or a fix for the worktree
`--resume` gap.

**claude-control** (github.com/sverrirsig/claude-control) is an Electron + Next.js dashboard that
discovers sessions by PID, classifies status via hooks, and integrates git/PR state and terminal
focus. Strong on visibility. It has no session notes, and because discovery is PID-based, a
session that dies simply disappears — the interrupted-work case is unaddressed.

Claudron borrows the discovery-and-dashboard concept and diverges on the two things that matter
most here: **surviving closure** (tmux) and **remembering intent** (notes).

## Architecture

**Load-bearing decision: tmux is the source of truth; Claudron owns only annotations.**

Claudron never persists which sessions exist. That is re-derived on every launch from:

- `tmux list-sessions` — managed sessions (Phase 2+)
- `~/.claude/projects/**/*.jsonl` — the global session index, including dead sessions

Claudron's own store holds only what no other system knows: notes, manual status, and display
names, keyed by `sessionId`. A Claudron crash is therefore a non-event — processes keep running
under the tmux server, relaunching re-adopts them, and at most the last unsaved note keystroke is
lost.

**Stack:** Tauri + React + TypeScript + Tailwind + shadcn; Zustand for session state, TanStack
Query for polling.

Tauri is chosen over Electron (claude-control's choice) because the Rust backend owns tmux control
mode and transcript indexing, keeping the JS heap holding only what is rendered. Indexing 273
project directories inside the same heap as the terminal buffers is precisely the memory profile
to avoid. Tauri also matches existing in-house Tauri experience (EncUiAws).

### Layers

| Layer | Responsibility | Depends on |
|---|---|---|
| **Discovery** (Rust) | Index transcripts; list tmux sessions; map PIDs→cwd for legacy sessions | filesystem, tmux, `ps`/`lsof` |
| **Annotation store** (Rust) | Notes, manual status, display names keyed by `sessionId`; write-on-change | filesystem only |
| **UI** (React) | Pane of glass, filtering, jump/attach actions, embedded terminal (Phase 3) | Tauri IPC only |

The UI never touches tmux or the filesystem directly. That boundary is what allows the embedded
terminal to arrive in Phase 3 without disturbing the other layers.

## Session model

A Claudron session is a **stable identity that outlives any particular process**. The `sessionId`
from the transcript is that identity; it survives the process dying, the tab closing, and Claudron
restarting.

### Liveness states

Computed fresh at read time, never persisted:

| State | Detection | Available actions |
|---|---|---|
| **Managed** | Has a tmux session | Attach in Claudron, eject to iTerm2, jump |
| **Legacy** | Live `claude` PID, no tmux (existing iTerm2 sessions) | Jump to iTerm2 tab |
| **Interrupted** | No process; transcript has `interruptedByShutdown` | Resume |
| **Idle** | No process; ended cleanly | Resume |

**Interrupted** is a distinct state but a rare one among interactive sessions (0 of 21 recent).
It is retained because detecting it is free once transcripts are parsed, and it is a useful signal
when it does occur — not because it is common.

### Auto-captured fields

Verified present in **60 of 60** most recent transcripts:

`sessionId` · `aiTitle` (display name) · `lastPrompt` · `gitBranch` · `cwd` · `version`

Plus last-activity time (file mtime) and two derived values:

- **project** — from `cwd`, with worktrees grouped under their parent repo and rendered as
  `api-service ▸ feature-work`. This is the fix for unreadable tab titles: a two-part label reads
  clearly where a truncated tab title does not.

  **Who actually uses worktrees.** Measured at implementation time: all **282** transcripts whose
  `cwd` is a worktree have `entrypoint: sdk-py` — programmatic subagent runs. **Zero** of the 37
  indexed interactive sessions live in a worktree. So worktree labelling is correct code that
  currently fires on nothing, and it is not, today, the fix for unreadable tab titles. What
  actually distinguishes the 37 real sessions is their parent repo (`api-service` ×9, `workspace` ×7,
  `web-ui` ×4, `enc-ui-aws` ×3, …), which the plain-repo branch of `project_label` already
  handles. The worktree branch is retained because it is tested, costs nothing, and starts
  mattering the moment an interactive session is opened inside a worktree.

  **Real worktree layouts on the target machine** (measured across 83 distinct worktree paths
  found in live transcripts) — an earlier draft of this spec assumed a `worktrees/` directory
  directly under the repo, which matches **none** of them:

  | Layout | Count | Example |
  |---|---|---|
  | `<repo>/.claude/worktrees/<name>` | 46 | `api-service/.claude/worktrees/ui-refresh` |
  | `<repo>/.worktrees/<name>` | 37 | `api-service/.worktrees/feature-work` |

  The `.claude/worktrees/` case is a **two-segment** marker: naively matching on the segment
  `worktrees` alone yields `.claude` as the repo name, which is wrong. The repo is the segment
  before `.claude`.
- **liveness** — per the table above.

### User-owned fields

- **Notes** — freeform markdown, autosaved
- **Manual status** — one of `blocked` · `needs-review` · `waiting-on-me` · `background` · none

Both persist across session death and Claudron restart, and are independent of liveness.

### Rules

- **Interactive sessions only.** Only transcripts containing `entrypoint: "cli"` are indexed;
  `sdk-py` transcripts are programmatic subagent runs, are not sessions the user sits in, and
  account for 95% of transcript files. This filter is the difference between a list of ~21 real
  sessions and a list of 400 mostly-noise entries.
- **Sidechains excluded.** Records with `isSidechain: true` are subagent transcripts and are not
  resumable; including them would flood the list.
- **Repeated records: take the last.** `last-prompt` and `ai-title` are appended repeatedly over a
  session's life. Parsers must use the final occurrence. Note that a `last-prompt` record may carry
  only a `leafUuid` with no `lastPrompt` field — such records must be skipped, not treated as an
  empty prompt.
- **Global index.** All project directories are indexed together. Resume launches with the correct
  `cwd`, so the worktree a session lived in never needs to be known.

## Phasing

### Phase 1 — Pane of glass (no workflow change)

Read-only index across all project directories, plus PID discovery for existing iTerm2 sessions.
Full list with auto-captured fields, notes, manual status, filtering, and search.

Two actions: **jump to iTerm2 tab** (AppleScript) and **resume** (`claude --resume <id>` in the
correct `cwd`).

Delivers the `--resume` gap fix and the interrupted-session list with zero change to how sessions
are started. Phase 1 is also the gate: if the pane of glass does not earn daily use, the later
phases are not worth building.

### Phase 2 — tmux-managed sessions

Claudron spawns new sessions inside tmux; closing an iTerm2 tab detaches rather than kills. Adds
**attach**, **eject to iTerm2** (`osascript` opening a tab running `tmux attach`), and inline
**approve/reject and quick reply** via `tmux send-keys`.

Phase 1 legacy sessions continue to work unchanged. They cannot be attached, and inline
approve/reject and quick reply are **unavailable** for them — there is no input channel to a
process Claudron did not spawn. Jump-to-tab remains their ceiling.

`history-limit` is capped at 5000 lines per session here, since that must be set at spawn time,
ahead of Phase 3's need for it.

### Phase 3 — Embedded terminal

xterm.js panes over tmux control mode, with these memory rules built in from the start:

- Only **visible** panes hold live xterm.js instances (plus 2–3 recently viewed).
- Non-visible panes are **disposed** and re-hydrated from `tmux capture-pane -p -S -3000` on
  switch.
- Memory therefore scales with *visible* panes (1–3), not *total* sessions (10–20).
- WebGL renderer; PTY stream on a direct channel, not through the React render loop.
- **"View full history" reads the JSONL transcript**, not terminal scrollback — deeper than either
  buffer and free.

Eject-to-iTerm2 remains permanent, not a stopgap.

### Accepted trade-offs of the embedded terminal

Recorded so they are not rediscovered later:

- Loss of iTerm2 features (semantic history, regex scrollback search, triggers, profiles,
  instant replay). Partially recoverable via xterm.js addons; the long tail is not.
- Copy/paste and selection fidelity are subtly worse than native.
- Input latency is higher than native — improvable to a few milliseconds, not eliminable.
- Scrollback beyond the tmux cap is unavailable in-terminal; mitigated by transcript-backed
  history.

The eject button is the standing mitigation for all of these.

## Out of scope

Deliberately excluded — present in claude-control, but scope rather than value here. Any may be
added later; none is load-bearing for the three stated problems.

PR/CI status rollups · Linear/MCP task extraction · desktop notifications · multi-monitor
targeting · worktree cleanup · cost and token tracking.

## Success criteria

1. Every live `claude` process appears in Claudron within **15 seconds** of launch — one 10 s
   poll interval plus scan time.
2. A session closed mid-work is recoverable in under 10 seconds, regardless of which worktree it
   lived in. Measured from the session already being listed; excludes poll latency.
3. Notes and manual status survive both session death and a Claudron restart.
4. With 15 sessions listed and 3 terminals visible, Claudron holds under 500 MB.
5. A warm poll — no transcripts changed since the last scan — completes in under 2 seconds, so
   polls never overlap.

### Correction to criterion 1

Criterion 1 originally read "within 5 seconds" and the poll interval was 3 seconds. Both were
written before the index scan was measured. A full cold scan of the real tree takes **~10 s**
(9.98 / 10.27 / 10.17 s across three runs over 1312 depth-2 transcripts, 1.0 GiB), so a 3 s poll
would have overlapped continuously and never settled. The poll is now 10 s, an mtime-keyed cache
makes warm scans cheap (criterion 5), and criterion 1 states the honest worst case.

## Testing approach

- **Discovery** — unit tests over fixture transcript directories, including worktree paths,
  sidechain records, and `interruptedByShutdown` files. Verified against the real 273-directory
  tree as an integration check.
- **Annotation store** — round-trip tests for write-on-change durability; simulated hard-crash
  (kill mid-write) must not corrupt the store.
- **tmux integration** (Phase 2) — tests against a real tmux server in a scratch socket, covering
  spawn, detach, re-adopt after restart, and eject.
- **Memory** (Phase 3) — an automated check asserting criterion 4 with 15 sessions and 3 visible
  panes.

## Error handling

- **tmux absent or not installed** — Phase 1 works fully; Phase 2/3 features are disabled with an
  explanatory state rather than an error. **This is the current state of the target machine:** tmux
  is not installed, so Phase 1 must be built and tested with tmux absent as the default case, not
  as a rare fallback.
- **Unreadable or malformed transcript** — that session is skipped with a logged warning; indexing
  continues. A single bad file must never break the list.
- **AppleScript/iTerm2 focus failure** — surfaced inline on the row; never fatal.
- **tmux server died with sessions recorded** — those sessions fall back to Idle/Interrupted and
  remain resumable.
