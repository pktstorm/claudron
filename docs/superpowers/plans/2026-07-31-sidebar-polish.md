# Sidebar Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Sort live sessions to the top of the sidebar, show each session's Claude Code version and highlight stale ones, and state plainly that the detail pane's status blocks are user-set.

**Architecture:** Three independent changes to existing code. The sort is one comparator in `index.rs`. The version baseline is computed in Rust during indexing (`max(installed binary, highest version observed)`) and shipped as a new field on the existing `list_sessions` response; the frontend only renders it. The status copy is a two-line edit.

**Tech Stack:** Rust 1.93 · Tauri 2.11 · React 19 · TypeScript 5 (strict) · Tailwind 4 · Vitest · yarn

## Global Constraints

- **Phase 2A is merged and green.** Do not regress it: 92 Rust tests (+6 ignored), 68 frontend tests, `clippy -- -D warnings` clean, `tsc --noEmit` clean.
- **No git or `gh` work.** The git tab is a separate spec. Nothing here shells out to git.
- **All filesystem/process access lives in Rust.** React calls Tauri commands only.
- **Rust module declarations go in `src-tauri/src/lib.rs` as `pub mod <name>;`**, never `main.rs` — `main.rs` is a thin shim calling `claudron_app_lib::run()`.
- **Nullable Rust `Option<T>` serializes as explicit JSON `null`** (no `skip_serializing_if` anywhere in the model), so TypeScript types use `T | null`, never `T?`.
- **No vitest `setupFiles`** — jest-dom matchers like `toBeInTheDocument()` are unavailable. Use `toBeDefined()` / `toBeNull()` / `toHaveLength()` / `toBe()`.
- **Use `??` for fallbacks, never `||`** — an empty string is a legitimate value that `||` would wrongly skip.
- **Version comparison is numeric per dotted segment, never string.** String comparison places `2.1.99` above `2.1.220`, which would invert the entire feature.

## Measured context

From the live machine on 2026-07-31. These numbers are why the work is worth doing:

| Metric | Value |
|---|---|
| Interactive sessions indexed | 37 |
| **Distinct Claude Code versions across them** | **16** |
| Sessions on the newest version (2.1.220) | 12 |
| Oldest version still present | 2.1.195 |

---

## File Structure

| File | Change |
|---|---|
| `src-tauri/src/version.rs` | **Create** — version parsing, comparison, and baseline resolution |
| `src-tauri/src/index.rs` | Modify — liveness-first sort comparator |
| `src-tauri/src/model.rs` | Modify — add `SessionList` wrapper carrying sessions plus the baseline |
| `src-tauri/src/commands.rs` | Modify — `list_sessions` returns `SessionList` |
| `src-tauri/src/lib.rs` | Modify — declare `pub mod version;` |
| `src/types.ts` | Modify — add `SessionList` type |
| `src/api/tauri.ts` | Modify — `listSessions` returns `SessionList` |
| `src/components/SessionRow.tsx` | Modify — render the version badge |
| `src/components/SessionDetail.tsx` | Modify — status heading and hint |
| `src/App.tsx` | Modify — unwrap the new response shape |

A new `version.rs` rather than folding into `index.rs`: version comparison is pure, self-contained logic with its own tests, and `index.rs` is already 490 lines.

---

### Task 1: Version comparison and baseline

**Files:**
- Create: `src-tauri/src/version.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod version;`)

**Interfaces:**
- Consumes: nothing
- Produces:
  - `version::parse(v: &str) -> Vec<u32>` — dotted string to numeric segments
  - `version::is_older(a: &str, b: &str) -> bool` — true when `a` precedes `b`
  - `version::installed() -> Option<String>` — `claude --version`, or None on failure
  - `version::baseline(installed: Option<String>, observed: &[String]) -> Option<String>`

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/version.rs`:

```rust
use std::process::Command;

/// Split a dotted version into numeric segments.
///
/// A segment that does not parse counts as 0 rather than panicking -- version
/// strings come from transcripts and are not guaranteed well-formed.
pub fn parse(_v: &str) -> Vec<u32> {
    Vec::new()
}

/// True when `a` is an earlier version than `b`.
pub fn is_older(_a: &str, _b: &str) -> bool {
    false
}

/// The version of the `claude` binary on PATH, if it can be determined.
pub fn installed() -> Option<String> {
    let out = Command::new("claude").arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // Output looks like "2.1.220 (Claude Code)" -- take the leading token.
    text.split_whitespace().next().map(str::to_string)
}

/// The version everything else is compared against.
pub fn baseline(_installed: Option<String>, _observed: &[String]) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dotted_segments() {
        assert_eq!(parse("2.1.220"), vec![2, 1, 220]);
        assert_eq!(parse("2.1"), vec![2, 1]);
    }

    #[test]
    fn an_unparseable_segment_counts_as_zero() {
        assert_eq!(parse("2.1.beta"), vec![2, 1, 0]);
        assert_eq!(parse(""), vec![0]);
    }

    #[test]
    fn compares_numerically_not_as_strings() {
        // The whole feature inverts if this is a string comparison:
        // "2.1.99" > "2.1.220" lexically, but 99 < 220 numerically.
        assert!(is_older("2.1.99", "2.1.220"));
        assert!(!is_older("2.1.220", "2.1.99"));
    }

    #[test]
    fn equal_versions_are_not_older() {
        assert!(!is_older("2.1.220", "2.1.220"));
    }

    #[test]
    fn compares_across_segment_counts() {
        assert!(is_older("2.1", "2.1.1"));
        assert!(!is_older("2.2", "2.1.9"));
    }

    #[test]
    fn baseline_prefers_whichever_is_newer() {
        // The binary can be newer than anything observed yet.
        assert_eq!(
            baseline(Some("2.1.220".into()), &["2.1.205".into()]),
            Some("2.1.220".into())
        );
        // And an observed session can be newer than a binary read at startup,
        // because Claude Code auto-updates while Claudron runs.
        assert_eq!(
            baseline(Some("2.1.220".into()), &["2.1.221".into()]),
            Some("2.1.221".into())
        );
    }

    #[test]
    fn baseline_falls_back_to_observed_when_the_binary_is_unknown() {
        assert_eq!(
            baseline(None, &["2.1.205".into(), "2.1.220".into()]),
            Some("2.1.220".into())
        );
    }

    #[test]
    fn baseline_is_none_when_there_is_nothing_to_compare() {
        assert_eq!(baseline(None, &[]), None);
    }
}
```

- [ ] **Step 2: Add `pub mod version;` to lib.rs and run tests to verify they fail**

Add `pub mod version;` to `src-tauri/src/lib.rs` in alphabetical position (after `transcript`).

Run: `cd src-tauri && cargo test version::`
Expected: 6 failures against the stubs; `an_unparseable_segment_counts_as_zero` and `baseline_is_none_when_there_is_nothing_to_compare` may pass vacuously.

- [ ] **Step 3: Implement**

Replace the three stubs (leave `installed()` as written):

```rust
pub fn parse(v: &str) -> Vec<u32> {
    v.split('.')
        .map(|seg| seg.parse::<u32>().unwrap_or(0))
        .collect()
}

pub fn is_older(a: &str, b: &str) -> bool {
    let (a, b) = (parse(a), parse(b));
    let len = a.len().max(b.len());
    for i in 0..len {
        // A missing segment is 0, so "2.1" precedes "2.1.1".
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x < y;
        }
    }
    false
}

pub fn baseline(installed: Option<String>, observed: &[String]) -> Option<String> {
    // Taking the max of both is what makes this self-correcting: Claude Code
    // auto-updates, so a value read once at startup can go stale while the app
    // runs. When a session appears carrying a newer version, that session is
    // itself proof a newer version exists.
    let mut best = installed;
    for v in observed {
        best = match best {
            None => Some(v.clone()),
            Some(cur) if is_older(&cur, v) => Some(v.clone()),
            Some(cur) => Some(cur),
        };
    }
    best
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test version::`
Expected: 8 tests pass.

- [ ] **Step 5: Verify against the real binary**

Add this ignored test to the `tests` module:

```rust
    #[test]
    #[ignore]
    fn reads_the_real_installed_version() {
        match installed() {
            Some(v) => {
                println!("installed claude version: {v}");
                assert!(!parse(&v).is_empty());
                assert!(v.chars().next().is_some_and(|c| c.is_ascii_digit()));
            }
            None => println!("claude binary not found or not runnable -- fallback path"),
        }
    }
```

Run: `cd src-tauri && cargo test version::tests::reads_the_real -- --ignored --nocapture`
Expected: prints the installed version. Report what it printed.

- [ ] **Step 6: Verify lint and commit**

```bash
cd src-tauri && cargo clippy -- -D warnings
git add src-tauri/src/version.rs src-tauri/src/lib.rs
git commit -m "feat: compare Claude Code versions and resolve a baseline"
```

---

### Task 2: Sort live sessions first

**Files:**
- Modify: `src-tauri/src/index.rs` (the `sort_by` at the end of `index_sessions`)

**Interfaces:**
- Consumes: `crate::model::Liveness`
- Produces: `index_sessions` ordering changes — live sessions precede interrupted, which precede idle; most-recent-first within each rank. No signature change.

- [ ] **Step 1: Write the failing tests**

Add to `src-tauri/src/index.rs`'s existing `tests` module:

```rust
    fn session_with(id: &str, liveness: Liveness, last_activity: i64) -> Session {
        Session {
            session_id: id.into(),
            ai_title: None,
            last_prompt: None,
            git_branch: None,
            cwd: "/repo".into(),
            project_label: "repo".into(),
            version: None,
            last_activity,
            liveness,
            annotation: Annotation::default(),
        }
    }

    #[test]
    fn live_sessions_sort_above_idle_ones_however_old() {
        // A session running right now must lead, even if it was started long
        // before an idle session that was touched seconds ago.
        let mut v = vec![
            session_with("idle-recent", Liveness::Idle, 9_000),
            session_with("live-old", Liveness::Legacy, 1),
        ];
        sort_sessions(&mut v);
        assert_eq!(v[0].session_id, "live-old");
    }

    #[test]
    fn interrupted_sorts_between_live_and_idle() {
        let mut v = vec![
            session_with("idle", Liveness::Idle, 9_000),
            session_with("interrupted", Liveness::Interrupted, 8_000),
            session_with("live", Liveness::Legacy, 7_000),
        ];
        sort_sessions(&mut v);
        let order: Vec<&str> = v.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(order, vec!["live", "interrupted", "idle"]);
    }

    #[test]
    fn most_recent_first_within_a_rank() {
        let mut v = vec![
            session_with("older", Liveness::Idle, 100),
            session_with("newer", Liveness::Idle, 900),
        ];
        sort_sessions(&mut v);
        assert_eq!(v[0].session_id, "newer");
    }

    #[test]
    fn managed_and_legacy_tie_so_activity_decides() {
        // Fixture deliberately puts the OLDER item on Managed. If Managed
        // ranked better than Legacy, rank alone would put it first and this
        // assertion would fail -- so passing proves the two genuinely tie and
        // activity is what decides. (A fixture where activity and a
        // hypothetical rank both favour the same winner proves nothing.)
        let mut v = vec![
            session_with("managed-older", Liveness::Managed, 100),
            session_with("legacy-newer", Liveness::Legacy, 900),
        ];
        sort_sessions(&mut v);
        assert_eq!(v[0].session_id, "legacy-newer");

        // And the mirror, so neither variant is favoured in either direction.
        let mut v = vec![
            session_with("legacy-older", Liveness::Legacy, 100),
            session_with("managed-newer", Liveness::Managed, 900),
        ];
        sort_sessions(&mut v);
        assert_eq!(v[0].session_id, "managed-newer");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test index::tests::live_sessions index::tests::interrupted_sorts index::tests::most_recent index::tests::managed_and_legacy`
Expected: compile error — `sort_sessions` does not exist.

- [ ] **Step 3: Implement**

Add this function to `src-tauri/src/index.rs`, above `index_sessions`:

```rust
/// Rank for sidebar ordering. Lower sorts first.
///
/// `Legacy` and `Managed` tie: both mean a live process, and how the session is
/// hosted is not a reason to rank one above the other.
fn liveness_rank(l: Liveness) -> u8 {
    match l {
        Liveness::Legacy | Liveness::Managed => 0,
        Liveness::Interrupted => 1,
        Liveness::Idle => 2,
    }
}

/// Live sessions first, then most-recently-active within each rank.
///
/// Extracted from `index_sessions` so the ordering can be tested without
/// building a transcript tree. Private -- the tests `use super::*`.
fn sort_sessions(sessions: &mut [Session]) {
    sessions.sort_by(|a, b| {
        liveness_rank(a.liveness)
            .cmp(&liveness_rank(b.liveness))
            .then(b.last_activity.cmp(&a.last_activity))
    });
}
```

Then replace the existing sort line at the end of `index_sessions`:

```rust
    sort_sessions(&mut out);
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test index::`
Expected: all index tests pass, including the 4 new ones and the pre-existing `sorts_most_recent_first`.

Note: `sorts_most_recent_first` uses fixtures that are all `Idle` or `Interrupted`. If it now fails, the fixtures span ranks and the test's assertion needs re-reading — report it rather than weakening the assertion.

- [ ] **Step 5: Verify lint and commit**

```bash
cd src-tauri && cargo clippy -- -D warnings
git add src-tauri/src/index.rs
git commit -m "feat: sort live sessions to the top of the sidebar"
```

---

### Task 3: Ship the baseline to the frontend

**Files:**
- Modify: `src-tauri/src/model.rs` (add `SessionList`)
- Modify: `src-tauri/src/commands.rs` (`list_sessions` returns it)
- Modify: `src/types.ts`, `src/api/tauri.ts`, `src/App.tsx`

**Interfaces:**
- Consumes: `version::{baseline, installed}`, `index::index_sessions`
- Produces:
  - Rust `model::SessionList { sessions: Vec<Session>, version_baseline: Option<String> }`
  - TS `SessionList { sessions: Session[]; versionBaseline: string | null }`
  - `listSessions(): Promise<SessionList>` (was `Promise<Session[]>`)

- [ ] **Step 1: Write the failing Rust test**

Add `SessionList` to `src-tauri/src/model.rs`:

```rust
/// The `list_sessions` response: the sessions plus the version everything is
/// compared against, so the client does not have to work it out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionList {
    pub sessions: Vec<Session>,
    pub version_baseline: Option<String>,
}
```

Add to `model.rs`'s `tests` module:

```rust
    #[test]
    fn session_list_serializes_to_camel_case() {
        let l = SessionList { sessions: vec![], version_baseline: Some("2.1.220".into()) };
        let j = serde_json::to_string(&l).unwrap();
        assert!(j.contains("\"versionBaseline\":\"2.1.220\""), "got {j}");
        assert!(j.contains("\"sessions\":[]"), "got {j}");
    }

    #[test]
    fn session_list_baseline_is_explicit_null_when_absent() {
        let l = SessionList { sessions: vec![], version_baseline: None };
        let j = serde_json::to_string(&l).unwrap();
        assert!(j.contains("\"versionBaseline\":null"), "got {j}");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test model::tests::session_list`
Expected: compile error — `SessionList` does not exist until you add it in Step 1. If you added it already, these pass; that is fine, the point is the shape is pinned.

- [ ] **Step 3: Return it from the command**

In `src-tauri/src/commands.rs`, change `assemble` and `list_sessions`. `assemble` keeps returning `Vec<Session>`; the command wraps it:

```rust
#[tauri::command]
pub fn list_sessions() -> SessionList {
    let live: Vec<String> = process::discover_claude_processes()
        .into_iter()
        .filter_map(|p| p.cwd)
        .collect();
    let sessions = assemble(&index::projects_root(), &annotations::store_path(), &live);
    let observed: Vec<String> = sessions.iter().filter_map(|s| s.version.clone()).collect();
    let version_baseline = crate::version::baseline(crate::version::installed(), &observed);
    SessionList { sessions, version_baseline }
}
```

Update the `use` line in `commands.rs` to import `SessionList` alongside the other model types.

- [ ] **Step 4: Run the whole backend suite**

Run: `cd src-tauri && cargo test`
Expected: all pass. Then `cargo clippy -- -D warnings` exits 0 and `cargo build` succeeds.

- [ ] **Step 5: Write the failing frontend test**

Add to `src/api/tauri.test.ts`:

```ts
  it("listSessions returns the session list wrapper", async () => {
    invoke.mockResolvedValue({ sessions: [], versionBaseline: "2.1.220" });
    const out = await listSessions();
    expect(invoke).toHaveBeenCalledWith("list_sessions");
    expect(out.versionBaseline).toBe("2.1.220");
    expect(out.sessions).toHaveLength(0);
  });
```

- [ ] **Step 6: Update the TS types and API**

In `src/types.ts`, add:

```ts
export interface SessionList {
  sessions: Session[];
  versionBaseline: string | null;
}
```

In `src/api/tauri.ts`, change the return type:

```ts
export function listSessions(): Promise<SessionList> {
  return invoke("list_sessions");
}
```

Import `SessionList` in that file's type import.

- [ ] **Step 7: Update App.tsx to unwrap the response**

`src/App.tsx` currently does `const sessions = data ?? [];`. Change to:

```tsx
  const sessions = data?.sessions ?? [];
  const versionBaseline = data?.versionBaseline ?? null;
```

Pass `versionBaseline` down to `SessionList` and on to each `SessionRow` — add it as a prop on both components (Task 4 renders it).

Update `src/App.test.tsx`'s `listSessions` mock: every `mockResolvedValue([...])` becomes `mockResolvedValue({ sessions: [...], versionBaseline: "2.1.220" })`. **There are exactly 5 call sites** — update all of them, or the App tests fail with `data.sessions` undefined.

Note the existing `mk()` helper in `SessionList.test.tsx` already defaults `version: "2.1.220"`, so pre-existing SessionList tests will start rendering a version badge once Task 4 lands. That is expected and harmless — none of them assert on the absence of other text. If one breaks, report it rather than changing the helper's default, since several tests rely on it.

- [ ] **Step 8: Run the whole frontend suite**

Run: `yarn vitest run` and `yarn tsc --noEmit`
Expected: all pass, tsc exits 0. Report the count.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat: ship the version baseline with the session list"
```

---

### Task 4: Render the version badge

**Files:**
- Modify: `src/components/SessionRow.tsx`
- Modify: `src/components/SessionList.tsx` (thread the prop)
- Test: `src/components/SessionList.test.tsx`

**Interfaces:**
- Consumes: `SessionList` prop threading from Task 3
- Produces: `SessionRow({ session, selected, onSelect, versionBaseline })` and `SessionList({ sessions, selectedId, onSelect, versionBaseline })`

- [ ] **Step 1: Write the failing tests**

Add to `src/components/SessionList.test.tsx`:

```tsx
  it("shows the session's Claude version", () => {
    render(
      <SessionList
        sessions={[mk({ version: "2.1.205" })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline="2.1.220"
      />,
    );
    expect(screen.getByText("v2.1.205")).toBeDefined();
  });

  it("renders no version badge when the session has none", () => {
    render(
      <SessionList
        sessions={[mk({ version: null })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline="2.1.220"
      />,
    );
    // Anchored to the exact badge format. A loose /^v/ would also match the
    // branch name or title and pass for the wrong reason.
    expect(screen.queryByText(/^v\d+\.\d+/)).toBeNull();
  });

  it("styles an outdated version differently from a current one", () => {
    const { rerender } = render(
      <SessionList
        sessions={[mk({ version: "2.1.205" })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline="2.1.220"
      />,
    );
    const stale = screen.getByText("v2.1.205").className;

    rerender(
      <SessionList
        sessions={[mk({ version: "2.1.220" })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline="2.1.220"
      />,
    );
    const current = screen.getByText("v2.1.220").className;

    expect(stale).not.toBe(current);
  });

  it("does not mark anything stale when there is no baseline", () => {
    const { rerender } = render(
      <SessionList
        sessions={[mk({ version: "2.1.205" })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline={null}
      />,
    );
    const noBaseline = screen.getByText("v2.1.205").className;

    rerender(
      <SessionList
        sessions={[mk({ version: "2.1.220" })]}
        selectedId={null}
        onSelect={vi.fn()}
        versionBaseline={null}
      />,
    );
    expect(screen.getByText("v2.1.220").className).toBe(noBaseline);
  });
```

The `mk()` helper in that file needs a `version` field if it lacks one — it is `Partial<Session>`-based, so passing `version` works as long as the base object includes it.

- [ ] **Step 2: Run to verify it fails**

Run: `yarn vitest run src/components/SessionList.test.tsx`
Expected: failures — `versionBaseline` is not a prop and no version renders.

- [ ] **Step 3: Add the comparison helper**

Create `src/version.ts`:

```ts
/// True when `a` is an earlier version than `b`.
///
/// Compares numerically per dotted segment. A string comparison would place
/// "2.1.99" above "2.1.220" and invert the whole feature.
export function isOlder(a: string, b: string): boolean {
  const pa = a.split(".").map((s) => Number.parseInt(s, 10) || 0);
  const pb = b.split(".").map((s) => Number.parseInt(s, 10) || 0);
  const len = Math.max(pa.length, pb.length);
  for (let i = 0; i < len; i += 1) {
    const x = pa[i] ?? 0;
    const y = pb[i] ?? 0;
    if (x !== y) return x < y;
  }
  return false;
}
```

with `src/version.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { isOlder } from "./version";

describe("isOlder", () => {
  it("compares numerically, not as strings", () => {
    expect(isOlder("2.1.99", "2.1.220")).toBe(true);
    expect(isOlder("2.1.220", "2.1.99")).toBe(false);
  });

  it("treats equal versions as not older", () => {
    expect(isOlder("2.1.220", "2.1.220")).toBe(false);
  });

  it("handles differing segment counts", () => {
    expect(isOlder("2.1", "2.1.1")).toBe(true);
    expect(isOlder("2.2", "2.1.9")).toBe(false);
  });

  it("treats an unparseable segment as zero", () => {
    expect(isOlder("2.1.beta", "2.1.1")).toBe(true);
  });
});
```

- [ ] **Step 4: Render the badge**

In `src/components/SessionRow.tsx`, import the helper and add the prop:

```tsx
import { isOlder } from "../version";
```

Change the signature to accept `versionBaseline: string | null`, and insert the badge into the existing header row, before the relative age:

```tsx
        <span className="flex shrink-0 items-center gap-1">
          {session.version && (
            <span
              className={`shrink-0 text-[10px] ${
                versionBaseline && isOlder(session.version, versionBaseline)
                  ? "text-amber-400"
                  : "text-neutral-600"
              }`}
            >
              v{session.version}
            </span>
          )}
          <span className="shrink-0 text-[10px] text-neutral-500">{relativeAge(session.lastActivity)}</span>
```

In `src/components/SessionList.tsx`, accept `versionBaseline: string | null` and pass it to each `SessionRow`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `yarn vitest run` and `yarn tsc --noEmit`
Expected: all pass including the 4 new SessionList tests and 4 new version tests.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: show each session's Claude version, highlighting stale ones"
```

---

### Task 5: State that the status blocks are user-set

**Files:**
- Modify: `src/components/SessionDetail.tsx`
- Test: `src/components/SessionDetail.test.tsx`

**Interfaces:**
- Consumes: nothing new
- Produces: no API change — copy only

- [ ] **Step 1: Write the failing test**

Add to `src/components/SessionDetail.test.tsx`:

```tsx
  it("says the status blocks are user-set, not detected", () => {
    render(<SessionDetail session={mk()} onAnnotationChange={vi.fn()} />);
    expect(screen.getByText("Your status")).toBeDefined();
    expect(screen.getByText(/not detected/i)).toBeDefined();
  });
```

- [ ] **Step 2: Run to verify it fails**

Run: `yarn vitest run src/components/SessionDetail.test.tsx`
Expected: FAIL — the heading currently reads "Status" and there is no hint.

- [ ] **Step 3: Update the copy**

In `src/components/SessionDetail.tsx`, replace the status section heading:

```tsx
      <section>
        <h3 className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-neutral-500">
          Your status
        </h3>
        <p className="mb-1 text-[11px] text-neutral-600">
          Set a label for yourself — this is not detected.
        </p>
        <StatusPicker value={a.status} onChange={(status) => onAnnotationChange({ ...a, status })} />
      </section>
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `yarn vitest run` and `yarn tsc --noEmit`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: say plainly that the status blocks are user-set"
```

---

### Task 6: Verify against real data

**Files:**
- Create: `docs/superpowers/plans/2026-07-31-sidebar-polish-verification.md`

**Interfaces:**
- Consumes: the complete application
- Produces: a written record of each success criterion, measured rather than asserted

- [ ] **Step 1: Confirm the full suite and lint**

Run each and record the number:
```bash
cd src-tauri && cargo test
cd src-tauri && cargo clippy -- -D warnings
yarn vitest run
yarn tsc --noEmit
yarn build
```

- [ ] **Step 2: Report the real baseline**

Run: `cd src-tauri && cargo test version::tests::reads_the_real -- --ignored --nocapture`
Record the printed installed version.

- [ ] **Step 3: Write the verification record**

Create `docs/superpowers/plans/2026-07-31-sidebar-polish-verification.md` with a table of the six spec criteria, the measured value or observation for each, and PASS / FAIL / NEEDS-GUI:

1. With at least one live session, that session appears at the top of the sidebar unfiltered.
2. Applying a filter that includes live sessions still lists them first.
3. Every row whose transcript carries a version displays it.
4. A below-baseline version is visually distinct from one on the baseline.
5. `2.1.99` is treated as older than `2.1.220`.
6. The detail pane states its status blocks are user-set.

Criteria 5 is covered by unit tests in both languages — cite them. Criteria 1–4 and 6 are visible in the running app; mark them NEEDS-GUI and write the exact steps a human should follow, in the style of `docs/superpowers/plans/2026-07-31-phase-2a-verification.md`.

- [ ] **Step 4: Commit**

```bash
git add docs
git commit -m "docs: record sidebar polish verification"
```

---

## Self-Review

**1. Spec coverage.** Every requirement maps to a task:

| Spec requirement | Task |
|---|---|
| Live sessions sort first | 2 |
| Legacy and Managed tie at rank 0 | 2 |
| Filtered results keep the ordering | 2 (inherited — frontend only filters) |
| Version rendered on each row | 4 |
| Below-baseline version highlighted | 4 |
| No badge when a session has no version | 4 |
| Baseline = max(installed, observed) | 1, 3 |
| Binary check falls back to observed | 1 |
| Numeric, not string, comparison | 1 (Rust), 4 (TS) |
| Row header layout, title truncates | 4 |
| Status heading says user-set | 5 |
| Criteria measured | 6 |

The comparison is implemented twice — once in Rust to compute the baseline, once in TS to decide the highlight. That is deliberate: shipping a per-session `isOutdated` boolean would put presentation logic in the backend, and the helper is six lines.

**2. Placeholder scan.** No TBDs, no "add error handling", no "similar to Task N". Every code step contains complete code.

**3. Type consistency.** `SessionList` matches between `model.rs` (camelCase via serde) and `types.ts`. `versionBaseline` is spelled identically in the Rust serde output, the TS type, the API wrapper, `App.tsx`, `SessionList.tsx`, and `SessionRow.tsx`. `sort_sessions(&mut [Session])` is defined in Task 2 and called only there. `isOlder(a, b)` in `src/version.ts` mirrors `version::is_older` in Rust with the same argument order and meaning.
