# Claudron — Git Tab

**Date:** 2026-07-31
**Status:** Approved, pending implementation plan

## Summary

A **Git** tab in the session detail slide-over, showing the state of the repository the
selected session is working in: branch and cleanliness, position relative to origin, whether
the directory is a worktree, and the pull request for the branch with its CI rollup. It also
carries the app's first destructive action — removing the session's worktree.

## Measured context

From the live machine on 2026-07-31. These numbers drove every design decision below.

| Operation | Cost |
|---|---|
| `git status --porcelain` | ~30 ms |
| `git rev-list --left-right --count HEAD...@{upstream}` | ~15 ms |
| `git worktree list` | ~85 ms |
| **`gh pr list --json …`** | **~835 ms** (network) |

| Repo | Worktrees |
|---|---|
| `api-service` | **73** |
| `workspace-nonsl-tf` | 23 |
| `web-ui` | 14 |
| `enc-ui-aws` | 13 |
| `toolkit` | 9 |
| `mobile-app` | 4 |

**136 worktrees across the machine.** Local git is cheap; `gh` is 25× more expensive and
hits the network. That gap is why refresh is on-demand rather than polled, and why the
remote block loads independently of the local ones.

Verified feasible before designing: `git rev-parse --is-inside-work-tree` identifies a
worktree, `--git-common-dir` resolves its parent repo, and `gh pr list` exposes
`headRefName`, which matches the session's `gitBranch`.

## Scope

**In:** the four read-only blocks below, plus removal of **the selected session's own
worktree**.

**Out — deliberately:** a cleanup view over all 136 worktrees. That is a repo-hygiene
feature, not a per-session one; bundling it here would make the tab do two unrelated jobs,
and a bulk-delete surface deserves its own design pass on confirmation and undo. Revisit if
the pile keeps growing.

## What the tab shows

Four blocks. Each loads independently, so a slow or failed `gh` call never blocks local data.

| Block | Content | Source |
|---|---|---|
| **Branch** | name, clean or dirty with changed-file count | `git status --porcelain` |
| **Origin** | ahead/behind counts, or "no upstream" | `git rev-list --left-right --count HEAD...@{upstream}` |
| **Worktree** | whether this cwd is one, its parent repo, Remove button | `git rev-parse --is-inside-work-tree`, `--git-common-dir` |
| **Pull request** | number, title, state, CI rollup | `gh pr list --json … --head <branch>` |

The pull-request block renders a loading state and fills in when `gh` returns. If `gh` is
missing or unauthenticated it says so, and the other three blocks are unaffected.

### No implicit fetch

Ahead/behind is computed against the last-fetched `origin/*`, so it can be stale. The block
says so rather than implying currency. Claudron does **not** run `git fetch`: it is slow, it
mutates the user's repository, and doing it silently on a read-only tab would be a surprise.
Honest staleness beats a hidden write.

**As shipped**, the block states "Compared against your last fetch" without a timestamp.
The original wording promised *when* the repo was last fetched; that was simplified during
implementation and the spec is amended here to match what exists. A timestamp would need
`.git/FETCH_HEAD`'s mtime, which is worth adding only if the static caveat proves too weak.

**Also as shipped:** ahead/behind is measured against `@{upstream}` — for a feature branch
that is `origin/<feature>`, not the default branch. The label must name the upstream, not
the default branch; conflating them understates divergence on the exact screen used to judge
merge readiness.

### Refresh model

Everything loads **on tab open**, with a manual refresh button. Nothing runs while the tab
is closed.

Polling was rejected deliberately. The git tab is consulted at a decision point — *is this
clean, did CI pass, can I delete this worktree* — not watched. Polling `gh` across 10+ open
sessions would also risk GitHub rate limits for little benefit. There is a correctness
argument too: with an explicit refresh you know exactly how fresh the data is, which matters
when the next action is deleting a worktree.

## Removing a worktree

The app's first destructive action, deliberately narrow.

- **Offered only when the session's cwd is itself a worktree.** Never for a main checkout.
- **Refused when the worktree is dirty.** Non-empty `git status --porcelain` disables the
  button and shows why. No `--force`, and no override in the UI.
- **Refused when a live process is in that directory.** Claudron already knows this from
  liveness; deleting there would pull the ground out from under a running session.
- **Confirmation names specifics** — the path, the branch, and whether that branch is merged
  into the repo's default branch — not a generic "are you sure?".

  The default branch is resolved as `git symbolic-ref --short refs/remotes/origin/HEAD`,
  falling back to `main` when that fails. The fallback is required, not defensive padding:
  measured on this machine, `api-service` and `web-ui` resolve it while **`claudron` itself
  has no `origin/HEAD` set**, so a resolver without a fallback would error on a real repo.
  If neither resolves, the confirmation omits the merged status rather than guessing — an
  unknown merge state must never be presented as "not merged", which would read as a
  warning that does not apply.
- **Runs `git worktree remove <path>`.** Never `rm -rf`. Git's own refusal conditions sit
  beneath Claudron's as a second net **for dirtiness and for main checkouts only**.

  **Git provides no second net for process occupancy**, and assuming otherwise caused the
  most serious defect on this branch. Verified on git 2.50.1 (Apple Git-155): with a live
  process cwd'd inside a worktree, `git worktree remove` exits 0 and deletes it — even when
  git itself is run from inside that worktree. Claudron's liveness guard is therefore the
  *only* thing protecting a running session, and it must match the worktree path **or any
  path beneath it**, not just the exact path.
- **Never deletes the branch.** Only the working directory. A branch is easy to recover;
  uncommitted work is not.

The confirmation states that the session becomes unresumable in that directory afterwards.

## Architecture

Rust performs all `git` and `gh` work; React renders. Same boundary as the rest of the app.

### New Rust module: `src-tauri/src/git/`

| File | Responsibility |
|---|---|
| `mod.rs` | The three Tauri commands |
| `run.rs` | Bounded subprocess execution — timeout, kill, capture |
| `local.rs` | Branch, cleanliness, ahead/behind, worktree identity |
| `remote.rs` | `gh pr list` invocation and CI rollup parsing |

`run.rs` exists because of a defect already found in this codebase: Phase 1's per-PID `lsof`
calls had no timeout, and a single hung call would have blocked the poll loop indefinitely.
Every `git` and `gh` invocation here goes through one bounded helper — **5 s for git, 15 s
for `gh`** — rather than each call site reinventing the guard.

### Three commands

Split so fast local data never waits on the network:

- `git_local(cwd: String) -> Result<GitLocal, String>` — branch, dirty count, ahead/behind,
  worktree identity
- `git_remote(cwd: String, branch: String) -> Result<GitRemote, String>` — PR number, title,
  state, CI rollup
- `remove_worktree(path: String) -> Result<(), String>` — the destructive one

### New React

`GitTab.tsx` plus a small component per block. The detail slide-over gains tabs — **Overview**
(today's content) and **Git**. That changes `SessionDetail`, so its existing tests remain the
guard that Overview is unharmed.

**As shipped:** one `GitTab.tsx` (~200 lines) rather than a component per block. At that size
the split would have been ceremony; revisit if the tab grows. The slide-over change landed in
`DetailSlideOver.tsx` and `SessionDetail.tsx` was left untouched — its tests stayed green
unmodified, which was the point.

**Tab state resets to Overview whenever the selected session changes**, and the slide-over
already closes on session change. Persisting the tab per session would mean opening the
slide-over on Git for a session whose repo data is stale and not yet loaded — the loading
state would be the first thing seen. Overview is always instant, so it is the safe default.

Because loading is on tab open, switching Overview → Git → Overview → Git re-runs the
commands each time. That is intended: it is the same freshness guarantee the refresh button
gives, and local git is ~130 ms for all three blocks.

## Error handling

Each case surfaces rather than failing silently:

- **Not a git repository** — the tab says so; no blocks render.
- **`gh` missing or unauthenticated** — the pull-request block explains; local blocks are
  unaffected.
- **No upstream** — the origin block says "no upstream", never `0/0`, which would read as
  "in sync".
- **Timeout** — that block shows a timeout with a retry; the others are unaffected.
- **Removal fails** — git's own stderr is shown verbatim, not paraphrased.

## Testing

- **Parsing is pure and unit-tested** against real captured output: porcelain status lines,
  `rev-list` counts, and `gh` JSON with mixed CI conclusions (success, failure, queued).
- **Subprocess behaviour** is tested against temp repositories created within the test —
  including a dirty worktree, a clean one, and one with no upstream.
- **`#[ignore]` integration tests** run against the real `api-service` checkout, which has 73
  worktrees and a live `gh` remote.
- **Frontend tests** cover each block's loading, loaded, and error states, and the removal
  guard rails: disabled when dirty, disabled when a process is live, confirmation content.

## Success criteria

1. The three local blocks render in **under 200 ms** on `api-service` (73 worktrees).
2. The pull-request block never blocks local rendering — local content is visible while
   `gh` is still in flight.
3. Removal is refused, with a visible reason, when the worktree is dirty **or** a live
   process occupies it.
4. `git worktree remove` is the only deletion mechanism. No `rm -rf` anywhere in the
   codebase.
5. Every subprocess is bounded by a timeout; no invocation can hang the UI.
6. With `gh` uninstalled or logged out, the local blocks still render correctly.
