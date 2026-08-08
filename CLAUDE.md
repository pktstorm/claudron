# Claudron — working notes for Claude Code

A macOS desktop app (Tauri 2 + React 19 + TypeScript + Tailwind 4) that indexes every Claude
Code session on the machine.

Everything here is a rule that was learned by getting it wrong. Where a rule cost real debugging
time, the cost is stated — the reason is usually the part that makes it stick.

## Commands

```bash
make dev        # Vite + Tauri, hot reload
make test       # both suites
make test-rust  # cargo test
make test-ui    # vitest
make lint       # clippy -D warnings, then tsc --noEmit
make build      # release .app bundle
```

`make test` and `make lint` must both pass before anything is considered done.

Tests marked `#[ignore]` run against a real repository:

```bash
CLAUDRON_TEST_REPO=~/code/some-repo \
  cargo test --manifest-path src-tauri/Cargo.toml -- --ignored --nocapture
```

## Architecture, in one rule

**Rust does all the work. React only renders.**

The UI never spawns a process, never reads a file, and never parses anything a subprocess
produced. It reaches the backend only through `src/api/`. If you find yourself wanting to shell
out from TypeScript, the function belongs in Rust with a Tauri command in front of it.

| Path | Responsibility |
|---|---|
| `src-tauri/src/transcript.rs` | Parse one `.jsonl` transcript into a session summary |
| `src-tauri/src/index.rs` | Walk project dirs, mtime-cached, produce sorted sessions |
| `src-tauri/src/conversation/` | Transcript → turns, tool calls, subagents |
| `src-tauri/src/git/` | Bounded `git`/`gh`, local inspection, PR and CI rollup |
| `src-tauri/src/process.rs` | Live `claude` processes and their directories |
| `src-tauri/src/annotations.rs` | Notes and status, stored atomically |
| `src-tauri/src/actions.rs` | Terminal focus and resume AppleScripts |
| `src/` | React UI |

Rust modules are declared in `lib.rs` (which holds the `tauri::Builder`), never in `main.rs`,
which is a thin shim. Rust tests live in an inline `#[cfg(test)] mod tests` at the bottom of the
file they test. Frontend tests sit beside their component as `Foo.test.tsx`.

## Testing

Write the failing test first. Run it. Confirm it fails **for the reason you expect** — a test
that fails because of a typo has told you nothing.

### The bar: would this test fail if the code were wrong?

This is the whole standard. A test that passes regardless of the implementation is worse than no
test, because it reads as coverage and stops anyone looking closer.

**Prove it, don't assume it.** Break the implementation deliberately — invert a condition, return
a constant, delete a guard — and confirm exactly the intended test fails. Then restore. Every
implementer on this project who did this found a real gap; several found gaps in tests that had
already passed review.

Five tests in this repo's history passed while the thing they claimed to cover was broken:

1. **Compared a value against itself.** A guard test built both sides of a path comparison from
   one string, so it could never detect that the two real sources disagree. The guard was broken;
   the test passed; it even survived mutation testing, because deleting the guard still failed
   it. *Rule: when a check compares data from two origins, build the test's inputs from two
   origins.*
2. **Asserted text another component rendered.** An App-level test for the Git tab asserted
   `getByText("main")`, which matched the always-mounted session list. Replacing the entire Git
   tab with a stub still passed. *Rule: assert on something only the component under test can
   render.*
3. **Asserted only that a call returned `Ok`.** True whether or not a pull request was found, so
   it proved the `gh` binary runs and nothing about parsing. *Rule: assert on the value, not just
   the shape.*
4. **Fixtures where two distinct fields held equal values.** `cwd` and `worktree.path` were the
   same in every fixture, so passing the wrong one to a destructive function was undetectable.
5. **Every fixture took the same branch.** `merged_into_default` had no test where the branch
   differed from the default, so the code path that shells out to git never ran.

### Test format

Name the behaviour, not the function. `a_nonzero_exit_carries_stderr_verbatim` beats
`test_run_error`. The name should read as the claim the test makes.

Assert on real captured output, not invented shapes. Fixtures in this repo mirror actual `git`
and `gh` output — including the awkward parts, like `rev-list` being tab-separated and a queued
check reporting an empty conclusion.

Failure messages should carry the value: `assert!(stderr.contains("bad things"), "got {stderr:?}")`.

Prefer real subprocesses and temp git repos over mocks when testing subprocess behaviour. That is
what is actually being tested.

### Never hardcode a live external identifier

A pull request number or branch name pinned today merges tomorrow, and the test then passes while
proving nothing. This rotted **twice within hours** here (PR #334 merged, re-pinned to #333, that
merged too). Discover a valid value at runtime, and make "none available" a loud skip rather than
a silent pass.

### Never mutate process-global state in a test

`cargo test` runs this crate's tests concurrently in one process. A test that cleared `PATH` to
prove a missing-binary path raced unrelated tests that shell out to `git`, failing about **one
run in seven** — invisible in a single run, found only by looping the suite twenty times.

Scope environment overrides to the child process, or avoid them. The fix that landed was better
than either: extract the error mapping into a pure function and test that directly, with no
subprocess at all.

**A single green run does not prove a race is absent.** When a change touches shared state,
threads, or subprocess spawning:

```bash
for i in $(seq 1 20); do
  cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=8 \
    || echo "FAILED ON $i"
done
```

### Frontend testing

**There is no vitest `setupFiles`.** `@testing-library/jest-dom` is in `package.json` but its
matchers are not registered — `toBeInTheDocument()` will fail confusingly. Use `toBeDefined()`,
`toBeNull()`, `toHaveLength()`, `toBe()`, `toEqual()`.

`App.tsx`'s `QueryClient` is a module-level singleton that is not cleared between tests, so a
render can briefly show a previous test's cached data. Wait on fixture-specific text rather than
text shared across fixtures. Tracked in #7.

## Rules that exist because of specific bugs

**Every subprocess goes through `src-tauri/src/git/run.rs`.** Never `std::process::Command`
directly. The runner enforces a timeout and drains both pipes on separate threads. A child that
writes past the pipe buffer while nothing reads it blocks forever — draining only after the
process exits deadlocks on any output above about 64 KB, which `git status` and `gh pr list`
both exceed in real repositories. Phase 1's `lsof` calls shipped with no timeout at all; a single
hung call would have blocked the poll loop indefinitely.

**Canonicalize both sides before comparing paths.** `lsof` reports fully-resolved paths
(`/private/tmp/...`) while recorded session directories are not resolved (`/tmp/...`). On macOS
these name the same directory and compare unequal. Raw string comparison let the worktree-removal
guard fail open and delete a directory out from under a running session.

**Safety guards belong in Rust, not in a disabled button.** A disabled control is an affordance;
anything invoking the command directly walks past it. Related: **git provides no second net for
process occupancy.** It refuses to remove a dirty worktree, but will happily remove one a live
process is sitting inside — verified, exit 0 — so Claudron's guard is the only protection.

**Nullable values are `T | null`, never `T?`.** Rust serializes `Option<T>` as an explicit `null`
(no `skip_serializing_if` anywhere in the models), and the distinction carries meaning:
`ahead: null` means "no upstream configured" while `ahead: 0` means "in sync". Rendering them the
same way tells the user something false. Likewise `mergedIntoDefault: null` means *unknown* and
must never render as "not merged".

**Use `??` for fallbacks, never `||`.** `0` and `""` are legitimate values that `||` discards.

**serde `rename_all` governs deserialization too.** A type that serialized as `inputTokens` then
silently failed to read source data written as `input_tokens`, and a `serde(default)` turned the
failure into a zero. Use field-level `alias` when reading external data.

**When data comes from an API, read the API's schema.** A sample from one repository tells you
what that repository does, not what the field can contain. `statusCheckRollup` is a union:
`CheckRun` (GitHub Actions) has `status`/`conclusion`, while `StatusContext` (CircleCI, Jenkins,
Codecov) has neither and reports `state`. A classifier reading only the first pair rendered a red
CircleCI build as "pending" — invisible on an Actions-only repo, where every test passed.

**Anything interpolated into a shell command must be quoted.** `resume_script` built
`cd {cwd} && claude --resume {id}` with only AppleScript escaping, so any path containing a space
broke `cd` — and because of `&&`, resume silently never ran. Session working directories and ids
come from transcript files, which are attacker-influenceable if a transcript is.

**Claudron never runs `git fetch`.** It is slow, it mutates the user's repository, and doing it
silently on a read-only tab would be a surprise. Ahead/behind is computed against the last-fetched
`origin/*` and the UI states that staleness. Honest staleness beats a hidden write.

**`git worktree remove` is the only deletion mechanism.** No `rm -rf`, no `remove_dir_all`, no
`--force`. The branch is never deleted — a branch is easy to recover, uncommitted work is not.

**`annotations::load` treats a corrupt store as an error, never as empty.** Returning empty would
let the next save atomically overwrite every existing note with one; the atomic write is exactly
what makes that clobber land cleanly.

## Documentation

Docs are part of the change, not a follow-up. A change that lands without them is incomplete.

**Update in the same commit as the code:**

- **`README.md`** — when a user-visible feature, requirement, or command changes. Its "Details
  that carry more weight than they look" section is where non-obvious behaviour goes.
- **`CONTRIBUTING.md`** — when a convention changes, or a new trap is worth warning about.
- **This file** — when you learn something the next agent would otherwise get wrong. That is the
  entire selection criterion.
- **`docs/superpowers/specs/`** — when the shipped behaviour diverges from what the spec
  describes. Amend the spec to match reality and say what changed; a spec that quietly disagrees
  with the code is worse than no spec, because it will be trusted.
- **Doc comments** — when a function's contract changes. Comments explaining *why* are load-bearing
  here; several encode bugs that would otherwise be reintroduced.

**Stale documentation is a defect.** If you notice a doc that no longer matches the code while
working nearby, fix it or file it — do not leave it to be discovered by someone trusting it.

## Working on this repo

**`main` is protected.** Changes land through a pull request with one approving review. Branch
from `main`, and never force-push it.

**Do not weaken or delete a test to make a suite pass.** If a test is wrong, say why in the pull
request and fix it deliberately.

**Report honestly.** If tests fail, say so with the output. If something is unverified, say that
rather than implying it was checked. Several defects here were caught only because an implementer
reported "this test does not actually discriminate" instead of counting it as coverage — that is
the standard, and it is more valuable than a clean-looking report.

**Say what you verified and how.** "Ran the suite, 163 passed" is useful. "Should work" is not.

**Scope discipline.** Fix what you were asked to fix. Pre-existing problems found along the way
get filed as issues, not folded into an unrelated diff — the pre-existing clippy warnings in #8
were deliberately left alone for exactly this reason.

**Measure claims about performance.** Local git inspection is 99.5–138.5 ms on a repo with 73
worktrees; `gh` is roughly 566 ms; a warm session scan is about 19 ms. State the range and how it
was measured, and do not overclaim what a number covers — that timing measures inspecting *one*
repo that has 73 worktrees, not enumerating all 73.

**Time the scan against a frozen copy of the transcript tree, never the live one.** The transcript
of the session doing the measuring is being appended while the test runs, so the warm scan takes a
real cache miss on it and reports an outlier — measured at ~26 ms against a 0.43 ms baseline, in
roughly **1 run in 7**. That looks exactly like the concurrency bug this file warns about elsewhere
and is not one. `cp -R ~/.claude/projects <tmp>` and point `CLAUDRON_PROJECTS_DIR` at the copy;
the same measurement went from a contaminated "+12% cold" to a stable +4.5%.

**The session walk and the dashboard walk are deliberately different walks.** Subagent transcripts
live at `<project>/<stem>/subagents/agent-<id>.jsonl` — depth 4, where the session walk stops at
depth 2. Statistics need them; the session list never reads them, and walking them took the file
count from 19 to 31 on a poll that runs continuously. So `walk` takes `include_subagents`. The trap
is the eviction rule that follows: a session-only walk never *visits* subagent transcripts, so
`cache.retain` must not read their absence from `seen` as deletion — doing so makes the 3-second
poll throw away the dashboard's cached statistics and forces a full re-parse every time it opens.
