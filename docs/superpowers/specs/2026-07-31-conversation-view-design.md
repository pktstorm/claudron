# Claudron Phase 2 — Conversation View and Reply Box

**Date:** 2026-07-31
**Status:** Approved, pending implementation plan
**Supersedes:** Phase 3 (embedded xterm.js terminal) in
`2026-07-30-claudron-design.md`, which is withdrawn — see "Why not a terminal".

## Summary

Claudron currently lists sessions and lets the user annotate, jump to, and resume them.
This phase adds the ability to **read a session's conversation inside Claudron**, rendered
from the transcript rather than from a terminal, and then to **reply to sessions Claudron
started**.

Split into two phases:

- **2A — conversation view.** Read-only. No tmux. Works on every session, live or long dead.
- **2B — tmux-backed sessions and reply box.** Opt-in per session.

**Each phase gets its own implementation plan.** 2A is independently useful and ships without
tmux; 2B depends on 2A and should not be planned until 2A is built and its rendering has been
used in earnest. This spec covers both so the shape of 2B constrains 2A's design — but the
2B section is deliberately lighter, and will need its own design pass on the questions tmux
raises (session naming, detach/attach semantics, what happens when the tmux server dies).

## Why not a terminal

The original design called for an embedded xterm.js terminal over a tmux pane. That is
withdrawn. The Claude Code desktop app does not render a terminal for code sessions; it
renders the *conversation*, and the conversation is a richer data structure that merely
happened to be displayed in a terminal.

Rendering the transcript instead of the terminal wins on five counts:

| | xterm.js over tmux | Conversation view |
|---|---|---|
| Data source | PTY byte stream | the JSONL already parsed for the session list |
| Requires tmux | Yes, mandatory | No |
| Works on ended sessions | No | **Yes** |
| Scrollback memory | A JS-heap buffer per pane | A file read by byte range |
| Collapsible tool calls, error styling, cost | Impossible — it is a character grid | Native |

The one thing a terminal provides that a transcript cannot is **input**. That is why 2B
exists, and why it uses `tmux send-keys` rather than a terminal emulator: the multiplexer is
needed for the write channel only, not for display.

### Input requires tmux — verified, not assumed

Live sessions have stdin bound to a TTY owned by iTerm2 (`lsof` on a real live session shows
fd 0/1/2 → `/dev/ttys000`). There is no supported way to inject input into another process's
TTY from outside it on modern macOS; `TIOCSTI` is disabled precisely to prevent this. A reply
channel therefore requires a multiplexer that owns the PTY from the start.

**Consequence, accepted:** a session already running in an iTerm2 tab can never gain a reply
box. Processes cannot be adopted into a tmux server after the fact. The fleet is permanently
mixed, and the UI must make that legible rather than treat it as a defect.

## Measured facts this design rests on

All figures taken from the live machine on 2026-07-31, not estimated.

| Metric | Value |
|---|---|
| Transcript files (depth 2) | 1338, **518 MB** total |
| Largest single transcript | **68.7 MB / 27,930 lines** |
| Interactive (`cli`) sessions indexed | 37 |
| Subagent transcript files | 1578 |
| `input.description` labels available for tool calls | 1212 in one sample |
| Subagent files linkable to their spawning call | 45 of 48 in one session |
| `parentUuid` records linking to a parent | 6063 of 6066 |
| tmux installed | **No** |

The 68.7 MB figure is why parsing belongs in Rust. Reading that into a renderer heap to
display the last twenty turns is not viable.

## Phase 2A — the conversation view

### What renders

Every element below was verified present in real transcripts.

| Element | Source |
|---|---|
| Prose | `text` content blocks |
| Tool call, collapsed to one line | `tool_use` — `name` plus `input.description` |
| Expanded tool output | `tool_result.content`, paired by `tool_use_id` |
| Error styling | `tool_result.is_error` |
| Ordering | `parentUuid` chain |
| Turn timing | `timestamp` deltas |
| Model and token cost | `message.model`, `message.usage` |
| Nested subagent turns | `subagents/agent-<id>.jsonl` |

Two findings shape the rendering:

**Tool calls arrive one per assistant turn.** All 1789 assistant turns in the sample carried
exactly one `tool_use` block. The desktop app's "Ran 2 commands ›" is a *display* coalescing
of consecutive turns, not a stored grouping. Claudron must therefore coalesce consecutive
tool-call turns itself; it is a presentation choice, tunable, not a field to read.

**Subagent nesting is derivable.** Subagent transcripts live in a sibling directory,
`<session-uuid>/subagents/agent-<agentId>.jsonl`, and carry `agentId` and `slug`. The
`agentId` appears in the *tool result* of the `Agent` call that spawned it, giving a complete
chain: `tool_use_id` → `agentId` → subagent file. Given heavy subagent use, these render as
expandable nested blocks rather than being dropped.

Note that each transcript has a `.meta.json` **sidecar** beside it — 1605 sidecars against
1598 transcripts. Anything walking a `subagents/` directory must filter to `.jsonl`, or it
will treat `agent-<id>.meta.json` as a transcript and derive `<id>.meta` as the agent id.
The original survey counted only `.jsonl` files and so missed these.

### Layout

The conversation becomes the right-hand pane. Notes, manual status, and the jump/resume
actions move to a slide-over — still one interaction away, not removed.

**The existing `SessionDetail` component is preserved, not rewritten.** It moves into the
slide-over intact, keeping its notes editor, status picker, action buttons, and error
surfacing. Only its mounting point changes. This keeps Phase 1's tested behaviour — including
the annotation debounce and the action-error alert — rather than reimplementing it.

### Liveness

A session list poll costs ~10 s cold, which is fine for a list and useless for watching a
conversation. Three mechanisms close that gap:

**Tail by byte offset.** Each open conversation records where parsing stopped. A poll seeks
to that offset and parses only new bytes. Appending is the only normal mutation, so this is
sound. Guard: if the file has shrunk or its inode changed, discard the offset and re-read
from the start.

**A delta carries result-patches, not just new turns.** A tool call and its result are
separate records, and the result is written when the tool *finishes*. Measured across real
transcripts: **60% of tool calls (6726 of 11170) take longer than one second** between
`tool_use` and `tool_result`, against a 1-second poll. So for most calls on a live session
the result lands in a later poll than the call itself.

A delta that only appended new turns would therefore drop those results permanently, and the
call would render "no result" forever — indistinguishable from a genuinely orphaned call, on
the majority of calls, in the primary use case. `ConversationDelta` accordingly carries
`updates: Vec<ToolResultUpdate>`, the parser carries pending-call state across polls, and the
client patches results into turns it already holds.

**Two polling rates.** The session list keeps 10 s. The *open* conversation polls at ~1 s.
Only one conversation is open at a time and a tail read is a few KB, so this costs far less
than the existing list scan.

Polling applies to the open conversation regardless of the session's liveness. An ended
session simply returns empty deltas — cheap, and it means a session that comes back to life
starts updating without special handling. There is no need to gate polling on liveness, and
doing so would add a state to get wrong.

**Virtualize.** A 27,930-line transcript cannot all be in the DOM. Render a window around
the scroll position. Tool results collapse by default — a single `ls -la` result was already
multi-KB, and sixty expanded would recreate, through the back door, the very memory problem
that made xterm.js unattractive.

> **NOT BUILT IN 2A — carried into 2B as its first item.** The final whole-branch review
> found that `ConversationPane` renders `turns.map(...)` over every turn: ~9,335 of them on
> the largest real transcript. The requirement belonged to no single task, so no task-scoped
> review owned it, and success criterion 4 (memory) — which would have caught it — is one of
> the two criteria that need a GUI and were left unmeasured.
>
> The consequence is asymmetric and worth stating plainly: parsing the largest transcript
> takes 148 ms, and then the renderer does the thing the whole Rust-side design existed to
> avoid. The 148 ms figure gives false reassurance about how long a large session takes to
> open. It degrades rather than corrupts, so it did not block the 2A merge, but tool results
> collapsing by default is currently the *only* thing keeping the DOM bounded.

Auto-scroll sticks to the bottom while the user is at the bottom, releases the moment they
scroll up, and offers "jump to latest".

## Phase 2B — tmux sessions and the reply box

Claudron gains "New session", which spawns Claude Code inside a tmux session. Those sessions
get a reply box in the conversation view, sending via `tmux send-keys`. Closing an iTerm2 tab
attached to one detaches rather than kills it.

**Opt-in, not wholesale.** The user continues starting sessions however they like. Claudron
does not require adoption and must not nag.

**A mixed fleet is the permanent steady state.** Every session displays whether it is
driveable, and a read-only session explains why in one line rather than showing a disabled
box.

### What the reply box cannot do

A pending permission prompt is **not** a distinct record type in the transcript. The
transcript logs what happened, not "I am blocked awaiting a y/n." Record types observed are
`assistant`, `user`, `attachment`, `last-prompt`, `ai-title`, `queue-operation`, `mode`,
`permission-mode`, `pr-link`, `system`, `file-history-snapshot`, `file-history-delta`,
`frame-link` — none of which announce a pending prompt.

The reply box is therefore a **plain text channel**, not a prompt-aware control. Detecting
"blocked, waiting on you" needs a different signal (tmux pane content or a Claude Code hook)
and is out of scope for 2B.

`permission-mode` **is** recorded, so the session's mode (`auto` vs asking) can be displayed.
That is useful signal and costs nothing.

## Architecture

Parsing lives in Rust for the same reason indexing does: 518 MB of transcripts must not pass
through the renderer heap. Rust parses and shapes; React renders an already-structured,
compact payload and never opens a file.

### New Rust modules

| File | Responsibility |
|---|---|
| `conversation.rs` | Parse transcript records into ordered turns and blocks |
| `tail.rs` | Byte-offset tailing; detect truncation and inode change |
| `subagent.rs` | Discover `subagents/*.jsonl`; link each to its spawning `tool_use_id` |

### New Tauri commands

- `load_conversation(sessionId) -> Conversation` — initial read; returns turns plus a tail offset
- `poll_conversation(sessionId, offset) -> ConversationDelta` — turns since `offset`, plus a new offset
- `load_subagent(sessionId, agentId) -> Conversation` — lazy, on expand only

Subagents load lazily and deliberately: one session had 48 subagent files, and loading them
eagerly would make opening a conversation slow for no benefit.

### Reused unchanged

`transcript.rs` (the summary parser still serves the session list), `index.rs`,
`annotations.rs`, `process.rs`, `actions.rs`. The conversation view is additive and must not
disturb the working index path.

### New React components

`ConversationPane` (virtualized), `TurnBlock`, `ToolCallBlock` (collapsed/expanded),
`SubagentBlock` (lazy), and a slide-over hosting the notes and status displaced from the
detail pane.

## Error handling

- **Transcript deleted or unreadable mid-session** — inline error in the pane; stop polling
  that session rather than erroring once a second.
- **File truncated or replaced** (inode change) — discard the offset, re-read from the start.
  Never render a torn parse.
- **Malformed record** — skip that record, render the rest. One bad line must never blank the
  conversation. Mirrors the Phase 1 rule.
- **Subagent file missing for a linked `agentId`** — the nested block renders "subagent
  transcript unavailable"; the parent turn still renders. 3 of 48 did not link cleanly in the
  sample, so this is a real case.
- **Enormous single tool result** — truncate the rendered output with a byte count and a
  "show full output" affordance.

## Testing

- **`conversation.rs`** — fixtures covering text-only turns, tool call/result pairing,
  orphaned `tool_use` with no matching result (real: happens when a session is killed
  mid-call), and malformed lines.
- **`tail.rs`** — append-then-poll returns only new turns; truncation and inode change force a
  full re-read; polling an unchanged file returns an empty delta and does no parsing work.
- **`subagent.rs`** — linking via `agentId`, and the unlinkable case.
- **Integration** (`#[ignore]`) — parse the largest real transcript (68.7 MB) from
  `~/.claude/projects`, asserting turn counts are plausible and no records are silently
  dropped.
- **Frontend** — collapse/expand, auto-scroll sticking and releasing, and that a delta
  appends rather than re-rendering the whole list.

## Success criteria

1. Opening the largest real transcript (68.7 MB, 27,930 lines) renders in **under 1 second in a
   release build** — the profile the shipped app uses. Measured during implementation: 72 MB /
   9,335 turns parses in ~151 ms release, ~1.27 s debug. The original criterion omitted the
   build profile, which made a passing implementation look like a failure.
2. A warm poll of an unchanged conversation completes in **under 50 ms**.
3. A new turn appears in an open conversation **within 2 seconds** of reaching disk.
4. Memory stays **under 500 MB** with the largest conversation open. Phase 1 measured 198 MB.
5. Every record type present in real transcripts is either rendered or **explicitly and
   deliberately ignored** — enforced by a test that fails on an unrecognised type, so a new
   record type surfaces as a failure rather than a silently missing turn. The test must scan
   the **whole** corpus: a capped scan during verification passed while missing three real
   types, which is worse than failing, because it reads as verified.

   Sixteen types exist across all 1368 transcripts. Two are conversation (`assistant`,
   `user`); the rest are control plane. Three — `agent-name`, `relocated`, `worktree-state`
   — were found only when the scan was widened. All three carry session metadata (an agent's
   display name, a moved cwd, worktree bookkeeping) and no `message` field, so none is
   renderable conversation.

## Out of scope

Embedded terminal emulation (withdrawn); adopting already-running sessions into tmux
(impossible); prompt-aware reply controls (no transcript signal); cross-session conversation
search; editing or deleting transcript history.
