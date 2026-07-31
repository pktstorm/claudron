# Claudron Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a read-only "pane of glass" desktop app that indexes every interactive Claude Code session across all project directories, lets the user attach notes and a status to each, and provides one-click jump-to-iTerm2 and resume-in-correct-cwd.

**Architecture:** A Tauri app with a Rust backend and a React frontend. The Rust side owns all filesystem and process access: it indexes `~/.claude/projects/**/*.jsonl` into session records, discovers live `claude` PIDs, and persists user annotations to a JSON file. The React side is a pure consumer of Tauri IPC commands and never touches the filesystem. Session existence is always re-derived from disk; only annotations are persisted.

**Tech Stack:** Rust 1.93 · Tauri 2.11 · React 19 · TypeScript 5 (strict) · Vite · Tailwind 4 · shadcn/ui · Zustand · TanStack Query · Vitest · yarn

## Global Constraints

- **Phase 1 only.** No tmux code, no spawning, no embedded terminal. Phases 2–3 get their own plans.
- **tmux is NOT installed on the target machine.** Nothing in Phase 1 may require it or fail without it.
- **Interactive sessions only.** Index a transcript only if it contains a record with `entrypoint == "cli"`. Exclude `sdk-py`. This filter is load-bearing: 379 of 400 recent transcripts are `sdk-py`.
- **Exclude sidechains.** Skip transcripts whose records carry `isSidechain: true`.
- **Repeated records: take the last occurrence** of `last-prompt` and `ai-title`. A `last-prompt` record may have no `lastPrompt` field — skip those, do not treat as empty string.
- **tmux/PID/filesystem access lives in Rust only.** The React layer calls Tauri commands exclusively.
- **A single malformed transcript must never break indexing.** Log and skip.
- **Package manager is `yarn`** (matches existing in-house projects).
- **Transcript root is `~/.claude/projects`**, overridable via the `CLAUDRON_PROJECTS_DIR` env var so tests can point at fixtures.

---

## File Structure

**Rust backend (`src-tauri/src/`)**

| File | Responsibility |
|---|---|
| `main.rs` | Tauri entrypoint; registers commands and app state |
| `model.rs` | Core types: `Session`, `Liveness`, `ManualStatus`, `Annotation` |
| `transcript.rs` | Parse one `.jsonl` file into a `TranscriptSummary`; entrypoint/sidechain filtering |
| `index.rs` | Walk the projects root, parse each transcript, produce `Vec<Session>` |
| `project.rs` | Decode project-dir names and `cwd` into repo/worktree display labels |
| `process.rs` | Discover live `claude` PIDs and map them to working directories |
| `annotations.rs` | Load/save the annotation store (notes, status, display name) |
| `actions.rs` | Jump-to-iTerm2 (AppleScript) and resume-session command builders |
| `commands.rs` | Tauri command surface consumed by the frontend |

**React frontend (`src/`)**

| File | Responsibility |
|---|---|
| `main.tsx` | App bootstrap, QueryClient provider |
| `App.tsx` | Layout shell: list pane + detail pane |
| `api/tauri.ts` | Typed wrappers over Tauri `invoke` calls |
| `types.ts` | TS mirrors of the Rust model types |
| `labels.ts` | User-visible strings for the `Liveness` and `ManualStatus` enums, shared by SessionRow, FilterBar, and StatusPicker |
| `store/filters.ts` | Zustand store for search text and active filters |
| `components/SessionList.tsx` | The list of sessions, grouped by project |
| `components/SessionRow.tsx` | One row: title, project label, liveness, status |
| `components/SessionDetail.tsx` | Detail pane: metadata, notes editor, actions |
| `components/StatusPicker.tsx` | Manual status selector |
| `components/FilterBar.tsx` | Search + liveness/status filters |

---

### Task 1: Project scaffolding and model types

**Files:**
- Create: `package.json`, `vite.config.ts`, `tsconfig.json`, `index.html`, `src/main.tsx`, `src/App.tsx`
- Create: `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/build.rs`, `src-tauri/src/main.rs`
- Create: `src-tauri/src/model.rs`
- Create: `Makefile`

**Interfaces:**
- Consumes: nothing (first task)
- Produces: `model::Session`, `model::Liveness`, `model::ManualStatus`, `model::Annotation`, all `serde`-serializable with camelCase field names for the TS boundary.

- [ ] **Step 1: Scaffold the Tauri app**

Run:
```bash
cd /Users/sthirlwall/code/claudron
yarn create tauri-app claudron-app --template react-ts --manager yarn
```

When it finishes, move its contents into the repo root (the repo already exists and has `docs/`):
```bash
shopt -s dotglob && mv claudron-app/* . && rmdir claudron-app
```

- [ ] **Step 2: Pin dependency versions**

In `src-tauri/Cargo.toml`, set the dependencies block exactly:

```toml
[dependencies]
tauri = { version = "2.11", features = [] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
walkdir = "2"
dirs = "5"
chrono = { version = "0.4", features = ["serde"] }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Write the failing test for model serialization**

Create `src-tauri/src/model.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Liveness {
    Managed,
    Legacy,
    Interrupted,
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ManualStatus {
    Blocked,
    NeedsReview,
    WaitingOnMe,
    Background,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub status: Option<ManualStatus>,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub session_id: String,
    pub ai_title: Option<String>,
    pub last_prompt: Option<String>,
    pub git_branch: Option<String>,
    pub cwd: String,
    pub project_label: String,
    pub version: Option<String>,
    pub last_activity: i64,
    pub liveness: Liveness,
    pub annotation: Annotation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_serializes_to_camel_case() {
        let s = Session {
            session_id: "abc".into(),
            ai_title: Some("Title".into()),
            last_prompt: None,
            git_branch: Some("main".into()),
            cwd: "/tmp".into(),
            project_label: "repo".into(),
            version: Some("2.1.220".into()),
            last_activity: 1234,
            liveness: Liveness::Idle,
            annotation: Annotation::default(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"sessionId\":\"abc\""));
        assert!(json.contains("\"lastActivity\":1234"));
        assert!(json.contains("\"liveness\":\"idle\""));
    }

    #[test]
    fn manual_status_round_trips() {
        let j = serde_json::to_string(&ManualStatus::NeedsReview).unwrap();
        assert_eq!(j, "\"needsReview\"");
        let back: ManualStatus = serde_json::from_str(&j).unwrap();
        assert_eq!(back, ManualStatus::NeedsReview);
    }
}
```

- [ ] **Step 4: Register the module**

In `src-tauri/src/lib.rs`, add `pub mod model;` at the top. The Tauri scaffold
puts the builder in `lib.rs`; `main.rs` is only a shim and must stay untouched.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri && cargo test model::`
Expected: 2 tests pass.

- [ ] **Step 6: Add the Makefile**

Create `Makefile` at the repo root:

```makefile
.PHONY: dev build test test-rust test-ui lint

dev:
	yarn tauri dev

build:
	yarn tauri build

test: test-rust test-ui

test-rust:
	cd src-tauri && cargo test

test-ui:
	yarn vitest run

lint:
	cd src-tauri && cargo clippy -- -D warnings
	yarn tsc --noEmit
```

- [ ] **Step 7: Verify the app builds and runs**

Run: `make dev`
Expected: a Tauri window opens with the default template page. Close it.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat: scaffold Tauri app and core model types"
```

---

### Task 2: Transcript parsing

**Files:**
- Create: `src-tauri/src/transcript.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod transcript;`)

**Interfaces:**
- Consumes: nothing from earlier tasks
- Produces: `transcript::TranscriptSummary { session_id: String, ai_title: Option<String>, last_prompt: Option<String>, git_branch: Option<String>, cwd: Option<String>, version: Option<String>, interrupted: bool }` and `transcript::parse_transcript(path: &Path) -> Option<TranscriptSummary>`. Returns `None` when the transcript is not an interactive session, is a sidechain, or is unreadable.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/transcript.rs` with tests first:

```rust
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TranscriptSummary {
    pub session_id: String,
    pub ai_title: Option<String>,
    pub last_prompt: Option<String>,
    pub git_branch: Option<String>,
    pub cwd: Option<String>,
    pub version: Option<String>,
    pub interrupted: bool,
}

pub fn parse_transcript(_path: &Path) -> Option<TranscriptSummary> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_jsonl(lines: &[&str]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for l in lines {
            writeln!(f, "{}", l).unwrap();
        }
        f.flush().unwrap();
        f
    }

    #[test]
    fn parses_a_cli_session() {
        let f = write_jsonl(&[
            r#"{"type":"user","entrypoint":"cli","sessionId":"s1","cwd":"/repo","gitBranch":"main","version":"2.1.220","isSidechain":false}"#,
            r#"{"type":"ai-title","aiTitle":"Fix the parser","sessionId":"s1"}"#,
            r#"{"type":"last-prompt","lastPrompt":"do the thing","sessionId":"s1"}"#,
        ]);
        let s = parse_transcript(f.path()).expect("should parse");
        assert_eq!(s.session_id, "s1");
        assert_eq!(s.ai_title.as_deref(), Some("Fix the parser"));
        assert_eq!(s.last_prompt.as_deref(), Some("do the thing"));
        assert_eq!(s.git_branch.as_deref(), Some("main"));
        assert_eq!(s.cwd.as_deref(), Some("/repo"));
        assert!(!s.interrupted);
    }

    #[test]
    fn rejects_sdk_sessions() {
        let f = write_jsonl(&[
            r#"{"type":"user","entrypoint":"sdk-py","sessionId":"s2","cwd":"/repo"}"#,
        ]);
        assert!(parse_transcript(f.path()).is_none());
    }

    #[test]
    fn rejects_transcripts_with_no_entrypoint() {
        let f = write_jsonl(&[r#"{"type":"user","sessionId":"s3","cwd":"/repo"}"#]);
        assert!(parse_transcript(f.path()).is_none());
    }

    #[test]
    fn rejects_sidechains() {
        let f = write_jsonl(&[
            r#"{"type":"user","entrypoint":"cli","sessionId":"s4","cwd":"/repo","isSidechain":true}"#,
        ]);
        assert!(parse_transcript(f.path()).is_none());
    }

    #[test]
    fn takes_the_last_title_and_prompt() {
        let f = write_jsonl(&[
            r#"{"type":"user","entrypoint":"cli","sessionId":"s5","cwd":"/repo"}"#,
            r#"{"type":"ai-title","aiTitle":"First title","sessionId":"s5"}"#,
            r#"{"type":"last-prompt","lastPrompt":"first prompt","sessionId":"s5"}"#,
            r#"{"type":"ai-title","aiTitle":"Second title","sessionId":"s5"}"#,
            r#"{"type":"last-prompt","lastPrompt":"second prompt","sessionId":"s5"}"#,
        ]);
        let s = parse_transcript(f.path()).unwrap();
        assert_eq!(s.ai_title.as_deref(), Some("Second title"));
        assert_eq!(s.last_prompt.as_deref(), Some("second prompt"));
    }

    #[test]
    fn ignores_last_prompt_records_without_a_prompt_field() {
        let f = write_jsonl(&[
            r#"{"type":"user","entrypoint":"cli","sessionId":"s6","cwd":"/repo"}"#,
            r#"{"type":"last-prompt","lastPrompt":"real prompt","sessionId":"s6"}"#,
            r#"{"type":"last-prompt","leafUuid":"abc","sessionId":"s6"}"#,
        ]);
        let s = parse_transcript(f.path()).unwrap();
        assert_eq!(s.last_prompt.as_deref(), Some("real prompt"));
    }

    #[test]
    fn detects_interruption() {
        let f = write_jsonl(&[
            r#"{"type":"user","entrypoint":"cli","sessionId":"s7","cwd":"/repo","interruptedByShutdown":true}"#,
        ]);
        assert!(parse_transcript(f.path()).unwrap().interrupted);
    }

    #[test]
    fn skips_malformed_lines_without_failing() {
        let f = write_jsonl(&[
            r#"{"type":"user","entrypoint":"cli","sessionId":"s8","cwd":"/repo"}"#,
            r#"this is not json at all"#,
            r#"{"type":"ai-title","aiTitle":"Survived","sessionId":"s8"}"#,
        ]);
        let s = parse_transcript(f.path()).unwrap();
        assert_eq!(s.ai_title.as_deref(), Some("Survived"));
    }

    #[test]
    fn returns_none_for_missing_file() {
        assert!(parse_transcript(Path::new("/nonexistent/x.jsonl")).is_none());
    }
}
```

- [ ] **Step 2: Add `pub mod transcript;` to lib.rs and run tests to verify they fail**

Run: `cd src-tauri && cargo test transcript::`
Expected: 8 failures (all assert against the `None` stub), 1 pass (`returns_none_for_missing_file`).

- [ ] **Step 3: Implement the parser**

Replace the `parse_transcript` stub in `src-tauri/src/transcript.rs`:

```rust
pub fn parse_transcript(path: &Path) -> Option<TranscriptSummary> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut summary = TranscriptSummary::default();
    let mut is_cli = false;

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        // A sidechain record anywhere disqualifies the whole transcript.
        if v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            return None;
        }

        if let Some(ep) = v.get("entrypoint").and_then(Value::as_str) {
            if ep == "cli" {
                is_cli = true;
            }
        }

        if let Some(id) = v.get("sessionId").and_then(Value::as_str) {
            if summary.session_id.is_empty() {
                summary.session_id = id.to_string();
            }
        }

        // `ai-title` and `last-prompt` repeat; the last one wins. A
        // `last-prompt` record may carry only a leafUuid and no prompt --
        // those must not overwrite a real value.
        if let Some(t) = v.get("aiTitle").and_then(Value::as_str) {
            summary.ai_title = Some(t.to_string());
        }
        if let Some(p) = v.get("lastPrompt").and_then(Value::as_str) {
            summary.last_prompt = Some(p.to_string());
        }
        if let Some(b) = v.get("gitBranch").and_then(Value::as_str) {
            summary.git_branch = Some(b.to_string());
        }
        if let Some(c) = v.get("cwd").and_then(Value::as_str) {
            summary.cwd = Some(c.to_string());
        }
        if let Some(ver) = v.get("version").and_then(Value::as_str) {
            summary.version = Some(ver.to_string());
        }
        if v.get("interruptedByShutdown").and_then(Value::as_bool) == Some(true) {
            summary.interrupted = true;
        }
    }

    if !is_cli || summary.session_id.is_empty() {
        return None;
    }
    Some(summary)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test transcript::`
Expected: 9 tests pass.

- [ ] **Step 5: Verify against real data**

Run:
```bash
cd src-tauri && cargo test transcript:: -- --nocapture
```
Then sanity-check the filter against the real tree with a throwaway binary check:
```bash
ls ~/.claude/projects/*/*.jsonl | wc -l
```
Expected: a large number (~1200+). The index test in Task 4 will confirm only a small fraction survive filtering.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: parse Claude Code transcripts into session summaries"
```

---

### Task 3: Project label decoding

**Files:**
- Create: `src-tauri/src/project.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod project;`)

**Interfaces:**
- Consumes: nothing
- Produces: `project::project_label(cwd: &str) -> String`, returning either `"repo"` or `"repo ▸ worktree"`.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/project.rs`:

```rust
/// Turn an absolute cwd into a short two-part display label.
///
/// Worktrees on this machine live in one of two layouts, measured across 83
/// real worktree paths:
///   `<repo>/.claude/worktrees/<name>`  (46) -- two-segment marker
///   `<repo>/.worktrees/<name>`         (37) -- single-segment marker
/// Both render as `api-service ▸ foo` rather than the unreadable truncation an
/// iTerm2 tab title gives. Matching the segment `worktrees` alone is WRONG
/// for the first layout -- it yields `.claude` as the repo name.
pub fn project_label(_cwd: &str) -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_repo_uses_its_directory_name() {
        assert_eq!(project_label("/Users/s/code/api-service"), "api-service");
    }

    #[test]
    fn worktree_is_labelled_under_its_parent_repo() {
        assert_eq!(
            project_label("/Users/s/code/api-service/worktrees/feature-work"),
            "api-service ▸ feature-work"
        );
    }

    #[test]
    fn dot_claude_worktrees_are_handled_too() {
        assert_eq!(
            project_label("/Users/s/code/api-service/.claude-worktrees/ui-refresh"),
            "api-service ▸ ui-refresh"
        );
    }

    // The four tests below use REAL paths taken from this machine's transcripts.
    // The two layouts above account for all 83 real worktree paths found.

    #[test]
    fn dot_worktrees_layout_is_labelled_under_its_parent_repo() {
        assert_eq!(
            project_label("/Users/sthirlwall/code/api-service/.worktrees/feature-work"),
            "api-service ▸ feature-work"
        );
    }

    #[test]
    fn claude_worktrees_layout_is_labelled_under_its_parent_repo() {
        assert_eq!(
            project_label("/Users/sthirlwall/code/api-service/.claude/worktrees/ui-refresh"),
            "api-service ▸ ui-refresh"
        );
    }

    #[test]
    fn claude_worktrees_does_not_report_dot_claude_as_the_repo() {
        let label =
            project_label("/Users/sthirlwall/code/admin-console/.claude/worktrees/agent-x");
        assert!(!label.starts_with(".claude"), "got {label}");
        assert_eq!(label, "admin-console ▸ agent-x");
    }

    #[test]
    fn short_worktree_paths_do_not_panic() {
        assert_eq!(project_label("/worktrees/b"), "b");
        assert_eq!(project_label("/a/.worktrees/b"), "a ▸ b");
    }

    #[test]
    fn trailing_slash_is_ignored() {
        assert_eq!(project_label("/Users/s/code/api-service/"), "api-service");
    }

    #[test]
    fn root_and_empty_degrade_gracefully() {
        assert_eq!(project_label("/"), "/");
        assert_eq!(project_label(""), "unknown");
    }
}
```

- [ ] **Step 2: Add `pub mod project;` to lib.rs and run tests to verify they fail**

Run: `cd src-tauri && cargo test project::`
Expected: 5 failures — every assertion gets an empty string.

- [ ] **Step 3: Implement**

Replace the `project_label` stub:

```rust
pub fn project_label(cwd: &str) -> String {
    let trimmed = cwd.trim_end_matches('/');
    if trimmed.is_empty() {
        return if cwd.starts_with('/') { "/".into() } else { "unknown".into() };
    }

    let parts: Vec<&str> = trimmed.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return "/".into();
    }

    let last = parts.len() - 1;
    let tree = parts[last];

    // Two-segment marker: `<repo>/.claude/worktrees/<name>` (46 real paths).
    // The repo is the segment BEFORE `.claude` -- matching `worktrees` alone
    // would report `.claude` as the repo.
    if parts.len() >= 4 && parts[last - 1] == "worktrees" && parts[last - 2] == ".claude" {
        return format!("{} ▸ {}", parts[last - 3], tree);
    }

    // Single-segment marker: `<repo>/.worktrees/<name>` (37 real paths).
    const WORKTREE_DIRS: [&str; 3] = [".worktrees", "worktrees", ".claude-worktrees"];
    if parts.len() >= 3 && WORKTREE_DIRS.contains(&parts[last - 1]) {
        return format!("{} ▸ {}", parts[last - 2], tree);
    }

    tree.to_string()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test project::`
Expected: 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: decode cwd into repo and worktree display labels"
```

---

### Task 4: Session indexing

**Files:**
- Create: `src-tauri/src/index.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod index;`)

**Interfaces:**
- Consumes: `transcript::parse_transcript`, `project::project_label`, `model::{Session, Liveness, Annotation}`
- Produces: `index::projects_root() -> PathBuf` and `index::index_sessions(root: &Path) -> Vec<Session>`, sorted by `last_activity` descending. Liveness is set to `Interrupted` or `Idle` only; live-process states are applied later in Task 6.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/index.rs`:

```rust
use crate::model::{Annotation, Liveness, Session};
use crate::project::project_label;
use crate::transcript::parse_transcript;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn projects_root() -> PathBuf {
    if let Ok(p) = std::env::var("CLAUDRON_PROJECTS_DIR") {
        return PathBuf::from(p);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join(".claude")
        .join("projects")
}

pub fn index_sessions(_root: &Path) -> Vec<Session> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn fixture_tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("-Users-s-code-repo");
        fs::create_dir_all(&proj).unwrap();

        let mut a = fs::File::create(proj.join("aaa.jsonl")).unwrap();
        writeln!(a, r#"{{"type":"user","entrypoint":"cli","sessionId":"aaa","cwd":"/Users/s/code/repo","gitBranch":"main"}}"#).unwrap();
        writeln!(a, r#"{{"type":"ai-title","aiTitle":"Session A","sessionId":"aaa"}}"#).unwrap();

        let mut b = fs::File::create(proj.join("bbb.jsonl")).unwrap();
        writeln!(b, r#"{{"type":"user","entrypoint":"sdk-py","sessionId":"bbb","cwd":"/Users/s/code/repo"}}"#).unwrap();

        let wt = dir.path().join("-Users-s-code-repo--worktrees-feat");
        fs::create_dir_all(&wt).unwrap();
        let mut c = fs::File::create(wt.join("ccc.jsonl")).unwrap();
        writeln!(c, r#"{{"type":"user","entrypoint":"cli","sessionId":"ccc","cwd":"/Users/s/code/repo/worktrees/feat","interruptedByShutdown":true}}"#).unwrap();

        dir
    }

    #[test]
    fn indexes_only_interactive_sessions() {
        let dir = fixture_tree();
        let sessions = index_sessions(dir.path());
        let ids: Vec<&str> = sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert!(ids.contains(&"aaa"));
        assert!(ids.contains(&"ccc"));
        assert!(!ids.contains(&"bbb"), "sdk-py session must be excluded");
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn applies_project_labels_including_worktrees() {
        let dir = fixture_tree();
        let sessions = index_sessions(dir.path());
        let c = sessions.iter().find(|s| s.session_id == "ccc").unwrap();
        assert_eq!(c.project_label, "repo ▸ feat");
        let a = sessions.iter().find(|s| s.session_id == "aaa").unwrap();
        assert_eq!(a.project_label, "repo");
    }

    #[test]
    fn marks_interrupted_sessions() {
        let dir = fixture_tree();
        let sessions = index_sessions(dir.path());
        let c = sessions.iter().find(|s| s.session_id == "ccc").unwrap();
        assert_eq!(c.liveness, Liveness::Interrupted);
        let a = sessions.iter().find(|s| s.session_id == "aaa").unwrap();
        assert_eq!(a.liveness, Liveness::Idle);
    }

    #[test]
    fn sorts_most_recent_first() {
        let dir = fixture_tree();
        let sessions = index_sessions(dir.path());
        for pair in sessions.windows(2) {
            assert!(pair[0].last_activity >= pair[1].last_activity);
        }
    }

    #[test]
    fn missing_root_yields_empty_not_panic() {
        assert!(index_sessions(Path::new("/nonexistent/claudron")).is_empty());
    }
}
```

- [ ] **Step 2: Add `pub mod index;` to lib.rs and run tests to verify they fail**

Run: `cd src-tauri && cargo test index::`
Expected: 4 failures (empty vec), 1 pass (`missing_root_yields_empty_not_panic`).

- [ ] **Step 3: Implement**

Replace the `index_sessions` stub:

```rust
pub fn index_sessions(root: &Path) -> Vec<Session> {
    let mut out = Vec::new();

    for entry in WalkDir::new(root)
        .max_depth(2)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }

        let Some(summary) = parse_transcript(path) else {
            continue;
        };

        let last_activity = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let cwd = summary.cwd.clone().unwrap_or_default();
        out.push(Session {
            session_id: summary.session_id,
            ai_title: summary.ai_title,
            last_prompt: summary.last_prompt,
            git_branch: summary.git_branch,
            project_label: project_label(&cwd),
            cwd,
            version: summary.version,
            last_activity,
            liveness: if summary.interrupted {
                Liveness::Interrupted
            } else {
                Liveness::Idle
            },
            annotation: Annotation::default(),
        });
    }

    out.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test index::`
Expected: 5 tests pass.

- [ ] **Step 5: Verify the filter against the real tree**

Add this ignored integration test to the `tests` module in `src-tauri/src/index.rs`:

```rust
    #[test]
    #[ignore]
    fn real_tree_filters_out_the_vast_majority() {
        let root = projects_root();
        if !root.exists() {
            return;
        }
        let total = WalkDir::new(&root)
            .max_depth(2)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("jsonl"))
            .count();
        let sessions = index_sessions(&root);
        println!("transcripts on disk: {}  indexed sessions: {}", total, sessions.len());
        assert!(sessions.len() < total / 2, "entrypoint filter should remove most transcripts");
    }
```

Run: `cd src-tauri && cargo test index::real_tree -- --ignored --nocapture`
Expected: prints both counts; indexed count is a small fraction of the total.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: index interactive sessions across all project directories"
```

---

### Task 5: Annotation store

**Files:**
- Create: `src-tauri/src/annotations.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod annotations;`)

**Interfaces:**
- Consumes: `model::{Annotation, ManualStatus}`
- Produces: `annotations::store_path() -> PathBuf`, `annotations::load(path: &Path) -> HashMap<String, Annotation>`, `annotations::save(path: &Path, map: &HashMap<String, Annotation>) -> std::io::Result<()>`. `save` writes atomically via a temp file plus rename.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/annotations.rs`:

```rust
use crate::model::Annotation;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn store_path() -> PathBuf {
    if let Ok(p) = std::env::var("CLAUDRON_STORE_PATH") {
        return PathBuf::from(p);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join(".claudron")
        .join("annotations.json")
}

pub fn load(_path: &Path) -> HashMap<String, Annotation> {
    HashMap::new()
}

pub fn save(_path: &Path, _map: &HashMap<String, Annotation>) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ManualStatus;

    #[test]
    fn round_trips_annotations() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("annotations.json");

        let mut map = HashMap::new();
        map.insert(
            "s1".to_string(),
            Annotation {
                notes: "check the migration".into(),
                status: Some(ManualStatus::Blocked),
                display_name: Some("Migration work".into()),
            },
        );
        save(&p, &map).unwrap();

        let loaded = load(&p);
        assert_eq!(loaded.get("s1").unwrap().notes, "check the migration");
        assert_eq!(loaded.get("s1").unwrap().status, Some(ManualStatus::Blocked));
    }

    #[test]
    fn missing_file_loads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(&dir.path().join("nope.json")).is_empty());
    }

    #[test]
    fn corrupt_file_loads_as_empty_rather_than_panicking() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bad.json");
        std::fs::write(&p, b"{ this is not json").unwrap();
        assert!(load(&p).is_empty());
    }

    #[test]
    fn save_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nested").join("deep").join("annotations.json");
        let map = HashMap::new();
        save(&p, &map).unwrap();
        assert!(p.exists());
    }

    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("annotations.json");
        save(&p, &HashMap::new()).unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains("tmp"))
            .collect();
        assert!(leftovers.is_empty(), "atomic save must clean up its temp file");
    }
}
```

- [ ] **Step 2: Add `pub mod annotations;` to lib.rs and run tests to verify they fail**

Run: `cd src-tauri && cargo test annotations::`
Expected: `round_trips_annotations` and `save_creates_missing_parent_directories` fail; the others pass against the stubs.

- [ ] **Step 3: Implement**

Replace the `load` and `save` stubs:

```rust
pub fn load(path: &Path) -> HashMap<String, Annotation> {
    let Ok(bytes) = std::fs::read(path) else {
        return HashMap::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn save(path: &Path, map: &HashMap<String, Annotation>) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Write to a sibling temp file and rename, so a crash mid-write cannot
    // leave a half-written store behind.
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(map)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test annotations::`
Expected: 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: persist session annotations with atomic writes"
```

---

### Task 6: Live process discovery

**Files:**
- Create: `src-tauri/src/process.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod process;`)

**Interfaces:**
- Consumes: nothing
- Produces: `process::LiveProcess { pid: i32, cwd: Option<String> }`, `process::discover_claude_processes() -> Vec<LiveProcess>`, and `process::parse_ps_output(out: &str) -> Vec<i32>` (split out so the parsing is testable without spawning `ps`).

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/process.rs`:

```rust
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub struct LiveProcess {
    pub pid: i32,
    pub cwd: Option<String>,
}

/// Extract PIDs of bare `claude` CLI processes from `ps -eo pid=,comm=` output.
///
/// Must match the CLI only -- the Claude desktop app and its Electron helpers
/// also match a naive "claude" substring search. Two independent filters:
/// an exact basename match (`claude`, not `Claude`), AND rejection of any path
/// containing `.app/Contents`. The case distinction alone is coincidental, and
/// its failure mode is a SILENT phantom session rather than a loud empty list,
/// so the bundle-path check is deliberate defense in depth.
pub fn parse_ps_output(_out: &str) -> Vec<i32> {
    Vec::new()
}

/// Resolve one pid's cwd via `lsof`, giving up after `timeout`.
///
/// Bounded deliberately: a hung `lsof` (classically, a stale network mount)
/// must never block the poll cycle. Uses std only -- spawn, then poll
/// `try_wait` until the child exits or the deadline passes, then kill and reap.
fn lsof_cwd_with_timeout(pid: i32, timeout: std::time::Duration) -> Option<String> {
    use std::io::Read;
    use std::process::Stdio;

    let mut child = Command::new("lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => return None,
        }
    }

    let mut text = String::new();
    child.stdout.as_mut()?.read_to_string(&mut text).ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix('n').map(str::to_string))
}

pub fn cwd_for_pid(pid: i32) -> Option<String> {
    lsof_cwd_with_timeout(pid, std::time::Duration::from_secs(2))
}

/// Discover live CLI sessions and their working directories.
///
/// This runs on every poll cycle (every 3s once Task 12 wires it up), so the
/// per-pid `lsof` calls MUST be bounded and concurrent: each is capped at 2s
/// via `lsof_cwd_with_timeout`, and one thread is spawned per pid. Sequential
/// un-timeout'd calls measured ~70ms each -- ~0.8s of blocking work at 12 live
/// sessions -- and a single hung `lsof` (stale network mount) would otherwise
/// block discovery indefinitely. Results are sorted by pid so the order is
/// deterministic.
pub fn discover_claude_processes() -> Vec<LiveProcess> {
    let Ok(out) = Command::new("ps").args(["-eo", "pid=,comm="]).output() else {
        return Vec::new();
    };
    let pids = parse_ps_output(&String::from_utf8_lossy(&out.stdout));

    let handles: Vec<_> = pids
        .into_iter()
        .map(|pid| std::thread::spawn(move || (pid, cwd_for_pid(pid))))
        .collect();

    let mut resolved: Vec<(i32, Option<String>)> =
        handles.into_iter().filter_map(|h| h.join().ok()).collect();
    resolved.sort_by_key(|(pid, _)| *pid);

    resolved
        .into_iter()
        .map(|(pid, cwd)| LiveProcess { pid, cwd })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = concat!(
        "  462 claude\n",
        "85346 claude\n",
        "39509 /Applications/Claude.app/Contents/MacOS/Claude\n",
        "40174 /Applications/Claude.app/Contents/Frameworks/Claude Helper.app/Contents/MacOS/Claude Helper\n",
        "42241 /Applications/Claude.app/Contents/Helpers/chrome-native-host\n",
        "12345 /opt/homebrew/bin/claude\n",
        "99999 zsh\n",
    );

    #[test]
    fn finds_bare_claude_cli_processes() {
        let pids = parse_ps_output(SAMPLE);
        assert!(pids.contains(&462));
        assert!(pids.contains(&85346));
    }

    #[test]
    fn finds_claude_invoked_by_absolute_path() {
        assert!(parse_ps_output(SAMPLE).contains(&12345));
    }

    #[test]
    fn excludes_the_desktop_app_and_its_helpers() {
        let pids = parse_ps_output(SAMPLE);
        assert!(!pids.contains(&39509), "desktop app must not be listed");
        assert!(!pids.contains(&40174), "Electron helper must not be listed");
        assert!(!pids.contains(&42241), "chrome native host must not be listed");
    }

    #[test]
    fn excludes_unrelated_processes() {
        assert!(!parse_ps_output(SAMPLE).contains(&99999));
    }

    #[test]
    fn empty_input_yields_no_pids() {
        assert!(parse_ps_output("").is_empty());
    }

    #[test]
    fn excludes_a_lowercase_claude_inside_an_app_bundle() {
        // Defense in depth: even if a bundled binary were named lowercase
        // `claude`, an .app/Contents path must never be treated as a CLI session.
        let out = "55555 /Applications/Claude.app/Contents/MacOS/claude\n";
        assert!(parse_ps_output(out).is_empty(), "app-bundle path must be excluded");
    }

    #[test]
    fn still_accepts_a_normal_cli_path() {
        let out = "12345 /opt/homebrew/bin/claude\n66666 claude\n";
        let pids = parse_ps_output(out);
        assert!(pids.contains(&12345));
        assert!(pids.contains(&66666));
    }

    #[test]
    fn lsof_timeout_returns_none_rather_than_hanging() {
        // A pid that cannot resolve must return None quickly, not block.
        let start = std::time::Instant::now();
        let got = lsof_cwd_with_timeout(999_999_9, std::time::Duration::from_secs(2));
        assert!(got.is_none());
        assert!(start.elapsed() < std::time::Duration::from_secs(5), "took {:?}", start.elapsed());
    }
}
```

- [ ] **Step 2: Add `pub mod process;` to lib.rs and run tests to verify they fail**

Run: `cd src-tauri && cargo test process::`
Expected: `finds_bare_claude_cli_processes` and `finds_claude_invoked_by_absolute_path` fail; the exclusion tests pass vacuously against the empty stub.

- [ ] **Step 3: Implement**

Replace the `parse_ps_output` stub:

```rust
pub fn parse_ps_output(out: &str) -> Vec<i32> {
    let mut pids = Vec::new();
    for line in out.lines() {
        let line = line.trim();
        let Some((pid_str, comm)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(pid) = pid_str.trim().parse::<i32>() else {
            continue;
        };
        // `comm` is the executable path. The CLI's basename is exactly
        // "claude"; the desktop app is "Claude" (capitalised) and its helpers
        // have longer basenames, so an exact basename match excludes them.
        let comm = comm.trim();
        let basename = comm.rsplit('/').next().unwrap_or(comm);
        if basename == "claude" {
            pids.push(pid);
        }
    }
    pids
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test process::`
Expected: 5 tests pass.

- [ ] **Step 5: Verify against the live machine**

Run:
```bash
ps -eo pid=,comm= | awk '{n=split($2,a,"/"); if (a[n]=="claude") print $1}'
```
Expected: several PIDs, none belonging to the Claude desktop app. Compare against `ps -eo pid,command | grep "[c]laude"` to confirm the desktop app's PIDs are absent.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: discover live claude CLI processes and their working directories"
```

---

### Task 7: Action builders

**Files:**
- Create: `src-tauri/src/actions.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod actions;`)

**Interfaces:**
- Consumes: nothing
- Produces: `actions::iterm_focus_script(cwd: &str) -> String`, `actions::resume_script(session_id: &str, cwd: &str) -> String`, `actions::run_applescript(script: &str) -> Result<(), String>`.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/actions.rs`:

```rust
use std::process::Command;

/// AppleScript to focus the iTerm2 tab whose session is running in `cwd`.
pub fn iterm_focus_script(_cwd: &str) -> String {
    String::new()
}

/// AppleScript to open a new iTerm2 tab and resume the given session in `cwd`.
pub fn resume_script(_session_id: &str, _cwd: &str) -> String {
    String::new()
}

pub fn run_applescript(script: &str) -> Result<(), String> {
    let out = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("failed to run osascript: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_script_targets_iterm_and_mentions_the_cwd() {
        let s = iterm_focus_script("/Users/s/code/repo");
        assert!(s.contains("iTerm"));
        assert!(s.contains("/Users/s/code/repo"));
    }

    #[test]
    fn resume_script_includes_the_session_id_and_cwd() {
        let s = resume_script("abc-123", "/Users/s/code/repo");
        assert!(s.contains("abc-123"));
        assert!(s.contains("/Users/s/code/repo"));
        assert!(s.contains("--resume"));
    }

    #[test]
    fn scripts_escape_embedded_double_quotes() {
        let s = resume_script("abc\"; do evil; \"", "/tmp");
        assert!(
            !s.contains("do evil; \""),
            "raw quote injection must not survive escaping"
        );
    }

    #[test]
    fn applescript_failure_is_reported_not_panicked() {
        let err = run_applescript("this is not valid applescript at all");
        assert!(err.is_err());
    }

    // The four tests below guard the SHELL boundary, which the AppleScript
    // escaping above does not cover.

    #[test]
    fn resume_script_shell_quotes_a_path_containing_spaces() {
        let s = resume_script("abc-123", "/Users/sam/Documents/My Project");
        assert!(
            s.contains(r"cd '/Users/sam/Documents/My Project'"),
            "path with spaces must be shell-quoted so cd does not break: {s}"
        );
    }

    #[test]
    fn resume_script_neutralizes_command_substitution() {
        let s = resume_script("abc-123", "/tmp/x$(touch /tmp/PWNED)");
        // Inside single quotes the shell does not expand $(...).
        assert!(s.contains(r"cd '/tmp/x$(touch /tmp/PWNED)'"), "got {s}");
    }

    #[test]
    fn resume_script_neutralizes_semicolon_chaining_in_session_id() {
        let s = resume_script("abc; touch /tmp/PWNED2", "/tmp");
        assert!(s.contains(r"--resume 'abc; touch /tmp/PWNED2'"), "got {s}");
    }

    #[test]
    fn shell_quote_handles_an_embedded_single_quote() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }
}
```

- [ ] **Step 2: Add `pub mod actions;` to lib.rs and run tests to verify they fail**

Run: `cd src-tauri && cargo test actions::`
Expected: the first three tests fail against empty-string stubs; `applescript_failure_is_reported_not_panicked` passes.

- [ ] **Step 3: Implement**

Replace the two stubs:

```rust
/// Escape a value for safe interpolation into an AppleScript string literal.
///
/// This guards ONE boundary. Any value that also reaches a shell (see
/// `resume_script`) must additionally go through `shell_quote`.
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Quote a value for a POSIX shell command line.
///
/// iTerm2's `write text` TYPES its argument into a shell, so any interpolated
/// value crosses two boundaries: the shell (inner, this function) and the
/// AppleScript string literal (outer, `esc`). Single quotes make the shell
/// treat everything literally; an embedded single quote is closed, escaped,
/// and reopened ('\'').
///
/// Without this, a cwd containing a space -- `~/Documents/My Project`, ordinary
/// on macOS -- breaks `cd`, and the `&&` means the resume never runs at all,
/// silently. A cwd containing `$(...)` executes.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn iterm_focus_script(cwd: &str) -> String {
    // Walk every tab and select the first whose working directory matches.
    format!(
        r#"tell application "iTerm2"
  activate
  repeat with w in windows
    tell w
      repeat with t in tabs
        tell t
          repeat with s in sessions
            if (variable named "session.path") of s is "{cwd}" then
              select w
              select t
              select s
              return "ok"
            end if
          end repeat
        end tell
      end repeat
    end tell
  end repeat
  return "not-found"
end tell"#,
        cwd = esc(cwd)
    )
}

pub fn resume_script(session_id: &str, cwd: &str) -> String {
    // Two boundaries: the shell (inner, shell_quote) and the AppleScript
    // string literal (outer, esc). Applying only esc here is a real bug --
    // see shell_quote's docs.
    let command = format!(
        "cd {} && claude --resume {}",
        shell_quote(cwd),
        shell_quote(session_id)
    );
    format!(
        r#"tell application "iTerm2"
  activate
  set newWindow to (create window with default profile)
  tell current session of newWindow
    write text "{command}"
  end tell
end tell"#,
        command = esc(&command)
    )
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test actions::`
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: build iTerm2 focus and session resume AppleScripts"
```

---

### Task 8: Tauri command surface

**Files:**
- Create: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/main.rs` (register commands and state)

**Interfaces:**
- Consumes: everything from Tasks 2–7
- Produces four Tauri commands the frontend calls by name:
  - `list_sessions() -> Vec<Session>` — indexed, annotated, liveness resolved
  - `set_annotation(sessionId: String, annotation: Annotation) -> Result<(), String>`
  - `focus_session(cwd: String) -> Result<(), String>`
  - `resume_session(sessionId: String, cwd: String) -> Result<(), String>`

- [ ] **Step 1: Write the failing test for annotation merging and liveness**

Create `src-tauri/src/commands.rs`:

```rust
use crate::actions;
use crate::annotations;
use crate::index;
use crate::model::{Annotation, Liveness, Session};
use crate::process;
use std::collections::HashMap;
use std::path::Path;

/// Merge annotations and live-process state into indexed sessions.
///
/// Split out from the Tauri command so it is testable without an app handle.
pub fn assemble(
    root: &Path,
    store: &Path,
    live_cwds: &[String],
) -> Vec<Session> {
    let _ = (root, store, live_cwds);
    Vec::new()
}

#[tauri::command]
pub fn list_sessions() -> Vec<Session> {
    let live: Vec<String> = process::discover_claude_processes()
        .into_iter()
        .filter_map(|p| p.cwd)
        .collect();
    assemble(&index::projects_root(), &annotations::store_path(), &live)
}

#[tauri::command]
pub fn set_annotation(session_id: String, annotation: Annotation) -> Result<(), String> {
    let path = annotations::store_path();
    let mut map = annotations::load(&path);
    map.insert(session_id, annotation);
    annotations::save(&path, &map).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn focus_session(cwd: String) -> Result<(), String> {
    actions::run_applescript(&actions::iterm_focus_script(&cwd))
}

#[tauri::command]
pub fn resume_session(session_id: String, cwd: String) -> Result<(), String> {
    actions::run_applescript(&actions::resume_script(&session_id, &cwd))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ManualStatus;
    use std::fs;
    use std::io::Write;

    fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("proj");
        fs::create_dir_all(&proj).unwrap();
        let mut f = fs::File::create(proj.join("s1.jsonl")).unwrap();
        writeln!(f, r#"{{"type":"user","entrypoint":"cli","sessionId":"s1","cwd":"/live/repo"}}"#).unwrap();
        let mut g = fs::File::create(proj.join("s2.jsonl")).unwrap();
        writeln!(g, r#"{{"type":"user","entrypoint":"cli","sessionId":"s2","cwd":"/dead/repo"}}"#).unwrap();
        let store = dir.path().join("annotations.json");
        (dir, store)
    }

    #[test]
    fn attaches_saved_annotations_to_sessions() {
        let (dir, store) = fixture();
        let mut map = HashMap::new();
        map.insert(
            "s1".to_string(),
            Annotation {
                notes: "remember this".into(),
                status: Some(ManualStatus::Blocked),
                display_name: None,
            },
        );
        annotations::save(&store, &map).unwrap();

        let sessions = assemble(dir.path(), &store, &[]);
        let s1 = sessions.iter().find(|s| s.session_id == "s1").unwrap();
        assert_eq!(s1.annotation.notes, "remember this");
        assert_eq!(s1.annotation.status, Some(ManualStatus::Blocked));
    }

    #[test]
    fn sessions_without_annotations_get_defaults() {
        let (dir, store) = fixture();
        let sessions = assemble(dir.path(), &store, &[]);
        let s2 = sessions.iter().find(|s| s.session_id == "s2").unwrap();
        assert_eq!(s2.annotation, Annotation::default());
    }

    #[test]
    fn a_live_cwd_marks_its_session_legacy() {
        let (dir, store) = fixture();
        let sessions = assemble(dir.path(), &store, &["/live/repo".to_string()]);
        let s1 = sessions.iter().find(|s| s.session_id == "s1").unwrap();
        assert_eq!(s1.liveness, Liveness::Legacy);
        let s2 = sessions.iter().find(|s| s.session_id == "s2").unwrap();
        assert_eq!(s2.liveness, Liveness::Idle);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Add `mod commands;` to `src-tauri/src/main.rs`.

Run: `cd src-tauri && cargo test commands::`
Expected: 3 failures — `assemble` returns an empty vec.

- [ ] **Step 3: Implement `assemble`**

Replace the `assemble` stub:

```rust
pub fn assemble(root: &Path, store: &Path, live_cwds: &[String]) -> Vec<Session> {
    let saved = annotations::load(store);
    index::index_sessions(root)
        .into_iter()
        .map(|mut s| {
            if let Some(a) = saved.get(&s.session_id) {
                s.annotation = a.clone();
            }
            // A live `claude` process in this session's cwd means the session
            // is running in a terminal Claudron did not spawn.
            if live_cwds.iter().any(|c| c == &s.cwd) {
                s.liveness = Liveness::Legacy;
            }
            s
        })
        .collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test commands::`
Expected: 3 tests pass.

- [ ] **Step 5: Register the commands**

The Tauri scaffold puts the builder in `lib.rs`, not `main.rs` — `main.rs` is a
thin shim calling `claudron_app_lib::run()`. Register the commands in `lib.rs`'s
existing `run()`, replacing the scaffold's `greet` registration, and delete the
now-unused `greet` function. Leave `main.rs` untouched.

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::list_sessions,
            commands::set_annotation,
            commands::focus_session,
            commands::resume_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

Note: `use std::collections::HashMap;` belongs INSIDE the `#[cfg(test)] mod tests`
block, not at the top of `commands.rs` — it is only used by tests, and `make lint`
runs clippy with `-D warnings`, so an unused import breaks linting.

- [ ] **Step 6: Run the full backend suite**

Run: `cd src-tauri && cargo test`
Expected: all tests across all modules pass.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: expose session listing and actions as Tauri commands"
```

---

### Task 9: Frontend types and API layer

**Files:**
- Create: `src/types.ts`, `src/api/tauri.ts`
- Create: `src/api/tauri.test.ts`
- Modify: `package.json` (add vitest), `vite.config.ts` (test config)

**Interfaces:**
- Consumes: the four Tauri commands from Task 8
- Produces: `Session`, `Liveness`, `ManualStatus`, `Annotation` TS types, and `listSessions()`, `setAnnotation()`, `focusSession()`, `resumeSession()` async functions.

- [ ] **Step 1: Add test tooling**

Run:
```bash
yarn add -D vitest @testing-library/react @testing-library/dom @testing-library/jest-dom jsdom
```

`@testing-library/dom` is a peer of `@testing-library/react` and is NOT installed
automatically. Without it, every component test in Tasks 10-12 fails at import time
with a "Require stack: @testing-library/react/dist/pure.js" error before any test runs.

Add to `vite.config.ts` inside `defineConfig({...})`:

```ts
  test: {
    environment: "jsdom",
    globals: true,
  },
```

- [ ] **Step 2: Write the types**

Create `src/types.ts`:

```ts
export type Liveness = "managed" | "legacy" | "interrupted" | "idle";

export type ManualStatus = "blocked" | "needsReview" | "waitingOnMe" | "background";

export interface Annotation {
  notes: string;
  status: ManualStatus | null;
  displayName: string | null;
}

export interface Session {
  sessionId: string;
  aiTitle: string | null;
  lastPrompt: string | null;
  gitBranch: string | null;
  cwd: string;
  projectLabel: string;
  version: string | null;
  lastActivity: number;
  liveness: Liveness;
  annotation: Annotation;
}
```

- [ ] **Step 3: Write the failing test for the API layer**

Create `src/api/tauri.test.ts`:

```ts
import { describe, it, expect, vi, beforeEach } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));

import { listSessions, setAnnotation, focusSession, resumeSession } from "./tauri";

describe("tauri api", () => {
  beforeEach(() => invoke.mockReset());

  it("listSessions calls the list_sessions command", async () => {
    invoke.mockResolvedValue([]);
    await listSessions();
    expect(invoke).toHaveBeenCalledWith("list_sessions");
  });

  it("setAnnotation passes sessionId and annotation", async () => {
    invoke.mockResolvedValue(undefined);
    const annotation = { notes: "n", status: null, displayName: null };
    await setAnnotation("s1", annotation);
    expect(invoke).toHaveBeenCalledWith("set_annotation", { sessionId: "s1", annotation });
  });

  it("focusSession passes the cwd", async () => {
    invoke.mockResolvedValue(undefined);
    await focusSession("/repo");
    expect(invoke).toHaveBeenCalledWith("focus_session", { cwd: "/repo" });
  });

  it("resumeSession passes sessionId and cwd", async () => {
    invoke.mockResolvedValue(undefined);
    await resumeSession("s1", "/repo");
    expect(invoke).toHaveBeenCalledWith("resume_session", { sessionId: "s1", cwd: "/repo" });
  });
});
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `yarn vitest run src/api/tauri.test.ts`
Expected: FAIL — `./tauri` does not exist.

- [ ] **Step 5: Implement the API layer**

Create `src/api/tauri.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";
import type { Annotation, Session } from "../types";

export function listSessions(): Promise<Session[]> {
  return invoke("list_sessions");
}

export function setAnnotation(sessionId: string, annotation: Annotation): Promise<void> {
  return invoke("set_annotation", { sessionId, annotation });
}

export function focusSession(cwd: string): Promise<void> {
  return invoke("focus_session", { cwd });
}

export function resumeSession(sessionId: string, cwd: string): Promise<void> {
  return invoke("resume_session", { sessionId, cwd });
}
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `yarn vitest run src/api/tauri.test.ts`
Expected: 4 tests pass.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: add typed frontend API layer over Tauri commands"
```

---

### Task 10: Filter store and session list

**Files:**
- Create: `src/store/filters.ts`, `src/store/filters.test.ts`
- Create: `src/components/SessionRow.tsx`, `src/components/SessionList.tsx`, `src/components/FilterBar.tsx`
- Create: `src/components/SessionList.test.tsx`

**Interfaces:**
- Consumes: `Session`, `Liveness`, `ManualStatus` from `src/types.ts`
- Produces: `useFilters()` Zustand store with `{ search, liveness, status, setSearch, setLiveness, setStatus }`; `applyFilters(sessions, filters) -> Session[]`; and the three components.

- [ ] **Step 1: Install dependencies**

Run:
```bash
yarn add zustand @tanstack/react-query
```

- [ ] **Step 2: Write the failing filter tests**

Create `src/store/filters.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { applyFilters, type Filters } from "./filters";
import type { Session } from "../types";

const base: Session = {
  sessionId: "s1",
  aiTitle: "Fix the parser",
  lastPrompt: "make it work",
  gitBranch: "main",
  cwd: "/code/repo",
  projectLabel: "repo",
  version: "2.1.220",
  lastActivity: 100,
  liveness: "idle",
  annotation: { notes: "", status: null, displayName: null },
};

const empty: Filters = { search: "", liveness: null, status: null };

describe("applyFilters", () => {
  it("returns everything when no filters are set", () => {
    expect(applyFilters([base], empty)).toHaveLength(1);
  });

  it("matches search against the title", () => {
    expect(applyFilters([base], { ...empty, search: "parser" })).toHaveLength(1);
    expect(applyFilters([base], { ...empty, search: "nomatch" })).toHaveLength(0);
  });

  it("matches search against the project label", () => {
    expect(applyFilters([base], { ...empty, search: "repo" })).toHaveLength(1);
  });

  it("matches search against notes", () => {
    const withNotes = { ...base, annotation: { ...base.annotation, notes: "check migration" } };
    expect(applyFilters([withNotes], { ...empty, search: "migration" })).toHaveLength(1);
  });

  it("search is case insensitive", () => {
    expect(applyFilters([base], { ...empty, search: "PARSER" })).toHaveLength(1);
  });

  it("filters by liveness", () => {
    expect(applyFilters([base], { ...empty, liveness: "legacy" })).toHaveLength(0);
    expect(applyFilters([base], { ...empty, liveness: "idle" })).toHaveLength(1);
  });

  it("filters by manual status", () => {
    const blocked = { ...base, annotation: { ...base.annotation, status: "blocked" as const } };
    expect(applyFilters([blocked], { ...empty, status: "blocked" })).toHaveLength(1);
    expect(applyFilters([base], { ...empty, status: "blocked" })).toHaveLength(0);
  });

  it("combines filters with AND", () => {
    const blocked = { ...base, annotation: { ...base.annotation, status: "blocked" as const } };
    expect(applyFilters([blocked], { search: "parser", liveness: "idle", status: "blocked" })).toHaveLength(1);
    expect(applyFilters([blocked], { search: "parser", liveness: "legacy", status: "blocked" })).toHaveLength(0);
  });
});
```

- [ ] **Step 3: Run to verify it fails**

Run: `yarn vitest run src/store/filters.test.ts`
Expected: FAIL — `./filters` does not exist.

- [ ] **Step 4: Implement the store**

Create `src/store/filters.ts`:

```ts
import { create } from "zustand";
import type { Liveness, ManualStatus, Session } from "../types";

export interface Filters {
  search: string;
  liveness: Liveness | null;
  status: ManualStatus | null;
}

interface FilterStore extends Filters {
  setSearch: (s: string) => void;
  setLiveness: (l: Liveness | null) => void;
  setStatus: (s: ManualStatus | null) => void;
}

export const useFilters = create<FilterStore>((set) => ({
  search: "",
  liveness: null,
  status: null,
  setSearch: (search) => set({ search }),
  setLiveness: (liveness) => set({ liveness }),
  setStatus: (status) => set({ status }),
}));

export function applyFilters(sessions: Session[], f: Filters): Session[] {
  const needle = f.search.trim().toLowerCase();
  return sessions.filter((s) => {
    if (f.liveness && s.liveness !== f.liveness) return false;
    if (f.status && s.annotation.status !== f.status) return false;
    if (!needle) return true;
    const haystack = [
      s.aiTitle ?? "",
      s.lastPrompt ?? "",
      s.projectLabel,
      s.gitBranch ?? "",
      s.annotation.notes,
      s.annotation.displayName ?? "",
    ]
      .join(" ")
      .toLowerCase();
    return haystack.includes(needle);
  });
}
```

- [ ] **Step 5: Run to verify it passes**

Run: `yarn vitest run src/store/filters.test.ts`
Expected: 8 tests pass.

- [ ] **Step 6: Write the failing component test**

Create `src/components/SessionList.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { SessionList } from "./SessionList";
import type { Session } from "../types";

const mk = (over: Partial<Session>): Session => ({
  sessionId: "s1",
  aiTitle: "Fix the parser",
  lastPrompt: "make it work",
  gitBranch: "main",
  cwd: "/code/repo",
  projectLabel: "repo",
  version: "2.1.220",
  lastActivity: 100,
  liveness: "idle",
  annotation: { notes: "", status: null, displayName: null },
  ...over,
});

describe("SessionList", () => {
  it("renders one row per session", () => {
    render(
      <SessionList
        sessions={[mk({ sessionId: "a" }), mk({ sessionId: "b", aiTitle: "Other work" })]}
        selectedId={null}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText("Fix the parser")).toBeDefined();
    expect(screen.getByText("Other work")).toBeDefined();
  });

  it("prefers the user's display name over the AI title", () => {
    render(
      <SessionList
        sessions={[mk({ annotation: { notes: "", status: null, displayName: "My name" } })]}
        selectedId={null}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText("My name")).toBeDefined();
  });

  it("shows the project label", () => {
    render(
      <SessionList
        sessions={[mk({ projectLabel: "api-service ▸ courier" })]}
        selectedId={null}
        onSelect={vi.fn()}
      />,
    );
    expect(screen.getByText("api-service ▸ courier")).toBeDefined();
  });

  it("shows the project label once, in the group header only", () => {
    render(
      <SessionList
        sessions={[
          mk({ sessionId: "a", projectLabel: "api-service ▸ courier" }),
          mk({ sessionId: "b", projectLabel: "api-service ▸ courier" }),
        ]}
        selectedId={null}
        onSelect={vi.fn()}
      />,
    );
    // Two sessions in one project must still yield exactly one label element.
    // Rendering it per row too would make getByText throw on the duplicate.
    expect(screen.getAllByText("api-service ▸ courier")).toHaveLength(1);
  });

  it("shows the git branch on the row", () => {
    render(
      <SessionList sessions={[mk({ gitBranch: "feat/courier" })]} selectedId={null} onSelect={vi.fn()} />,
    );
    expect(screen.getByText("feat/courier")).toBeDefined();
  });

  it("falls back to a placeholder when a session has no title", () => {
    render(
      <SessionList sessions={[mk({ aiTitle: null })]} selectedId={null} onSelect={vi.fn()} />,
    );
    expect(screen.getByText("Untitled session")).toBeDefined();
  });

  it("shows an empty state when there are no sessions", () => {
    render(<SessionList sessions={[]} selectedId={null} onSelect={vi.fn()} />);
    expect(screen.getByText(/no sessions/i)).toBeDefined();
  });
});
```

- [ ] **Step 7: Run to verify it fails**

Run: `yarn vitest run src/components/SessionList.test.tsx`
Expected: FAIL — `./SessionList` does not exist.

- [ ] **Step 8: Implement the components**

Create `src/components/SessionRow.tsx`:

```tsx
import type { Liveness, Session } from "../types";
// User-visible copy lives in one place -- see src/labels.ts. Three components
// need these strings; local copies drift.
import { LIVENESS_LABEL, STATUS_LABEL } from "../labels";

const LIVENESS_CLASS: Record<Liveness, string> = {
  managed: "bg-emerald-500/15 text-emerald-400",
  legacy: "bg-sky-500/15 text-sky-400",
  interrupted: "bg-amber-500/15 text-amber-400",
  idle: "bg-neutral-500/15 text-neutral-400",
};

export function SessionRow({
  session,
  selected,
  onSelect,
}: {
  session: Session;
  selected: boolean;
  onSelect: (id: string) => void;
}) {
  const title = session.annotation.displayName ?? session.aiTitle ?? "Untitled session";
  return (
    <button
      type="button"
      onClick={() => onSelect(session.sessionId)}
      className={`w-full border-l-2 px-3 py-2 text-left transition-colors ${
        selected
          ? "border-l-sky-400 bg-neutral-800"
          : "border-l-transparent hover:bg-neutral-800/50"
      }`}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="truncate text-sm font-medium text-neutral-100">{title}</span>
        <span className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] ${LIVENESS_CLASS[session.liveness]}`}>
          {LIVENESS_LABEL[session.liveness]}
        </span>
      </div>
      {/* No project label here -- SessionList already renders it as the group
          header, and repeating it per row is redundant in a narrow sidebar.
          The branch IS row-specific: sessions in one project differ by branch. */}
      {session.gitBranch && (
        <div className="mt-0.5 text-xs text-neutral-500">
          <span className="truncate">{session.gitBranch}</span>
        </div>
      )}
      {session.annotation.status && (
        <span className="mt-1 inline-block rounded bg-purple-500/15 px-1.5 py-0.5 text-[10px] text-purple-300">
          {/* Humanized via the shared map -- rendering the raw enum would show
              the user "waitingOnMe" in the sidebar. */}
          {STATUS_LABEL[session.annotation.status]}
        </span>
      )}
    </button>
  );
}
```

Create `src/components/SessionList.tsx`:

```tsx
import type { Session } from "../types";
import { SessionRow } from "./SessionRow";

export function SessionList({
  sessions,
  selectedId,
  onSelect,
}: {
  sessions: Session[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}) {
  if (sessions.length === 0) {
    return <div className="p-4 text-sm text-neutral-500">No sessions match the current filters.</div>;
  }

  const groups = new Map<string, Session[]>();
  for (const s of sessions) {
    const list = groups.get(s.projectLabel) ?? [];
    list.push(s);
    groups.set(s.projectLabel, list);
  }

  return (
    <div className="divide-y divide-neutral-800">
      {[...groups.entries()].map(([label, items]) => (
        <section key={label}>
          <h2 className="sticky top-0 bg-neutral-900/95 px-3 py-1 text-[11px] font-semibold uppercase tracking-wide text-neutral-500">
            {label}
          </h2>
          {items.map((s) => (
            <SessionRow
              key={s.sessionId}
              session={s}
              selected={s.sessionId === selectedId}
              onSelect={onSelect}
            />
          ))}
        </section>
      ))}
    </div>
  );
}
```

Create `src/components/FilterBar.tsx`:

```tsx
import type { Liveness, ManualStatus } from "../types";
import { useFilters } from "../store/filters";

const LIVENESS: Liveness[] = ["legacy", "interrupted", "idle"];
const STATUSES: ManualStatus[] = ["blocked", "needsReview", "waitingOnMe", "background"];

export function FilterBar() {
  const { search, liveness, status, setSearch, setLiveness, setStatus } = useFilters();
  return (
    <div className="space-y-2 border-b border-neutral-800 p-3">
      <input
        value={search}
        onChange={(e) => setSearch(e.target.value)}
        placeholder="Search sessions, notes, branches…"
        className="w-full rounded bg-neutral-800 px-2 py-1.5 text-sm text-neutral-100 outline-none placeholder:text-neutral-500 focus:ring-1 focus:ring-sky-500"
      />
      <div className="flex flex-wrap gap-1">
        {LIVENESS.map((l) => (
          <button
            key={l}
            type="button"
            onClick={() => setLiveness(liveness === l ? null : l)}
            className={`rounded px-2 py-0.5 text-[11px] ${
              liveness === l ? "bg-sky-500/20 text-sky-300" : "bg-neutral-800 text-neutral-400"
            }`}
          >
            {l}
          </button>
        ))}
        {STATUSES.map((s) => (
          <button
            key={s}
            type="button"
            onClick={() => setStatus(status === s ? null : s)}
            className={`rounded px-2 py-0.5 text-[11px] ${
              status === s ? "bg-purple-500/20 text-purple-300" : "bg-neutral-800 text-neutral-400"
            }`}
          >
            {s}
          </button>
        ))}
      </div>
    </div>
  );
}
```

- [ ] **Step 9: Run tests to verify they pass**

Run: `yarn vitest run src/`
Expected: all filter and SessionList tests pass.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "feat: add session list, row, and filter components"
```

---

### Task 11: Detail pane with notes and actions

**Files:**
- Create: `src/components/SessionDetail.tsx`, `src/components/StatusPicker.tsx`
- Create: `src/components/SessionDetail.test.tsx`

**Interfaces:**
- Consumes: `Session`, `Annotation`, `ManualStatus`; `focusSession`, `resumeSession` from `src/api/tauri.ts`
- Produces: `SessionDetail({ session, onAnnotationChange })` where `onAnnotationChange: (a: Annotation) => void`, and `StatusPicker({ value, onChange })`.

- [ ] **Step 1: Write the failing tests**

Create `src/components/SessionDetail.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import type { Session } from "../types";

const focusSession = vi.fn();
const resumeSession = vi.fn();
vi.mock("../api/tauri", () => ({
  focusSession: (...a: unknown[]) => focusSession(...a),
  resumeSession: (...a: unknown[]) => resumeSession(...a),
}));

import { SessionDetail } from "./SessionDetail";

const mk = (over: Partial<Session> = {}): Session => ({
  sessionId: "s1",
  aiTitle: "Fix the parser",
  lastPrompt: "make it work",
  gitBranch: "main",
  cwd: "/code/repo",
  projectLabel: "repo",
  version: "2.1.220",
  lastActivity: 100,
  liveness: "idle",
  annotation: { notes: "", status: null, displayName: null },
  ...over,
});

describe("SessionDetail", () => {
  beforeEach(() => {
    focusSession.mockReset();
    resumeSession.mockReset();
  });

  it("shows an empty state when nothing is selected", () => {
    render(<SessionDetail session={null} onAnnotationChange={vi.fn()} />);
    expect(screen.getByText(/select a session/i)).toBeDefined();
  });

  it("shows the last prompt and branch", () => {
    render(<SessionDetail session={mk()} onAnnotationChange={vi.fn()} />);
    expect(screen.getByText("make it work")).toBeDefined();
    expect(screen.getByText("main")).toBeDefined();
  });

  it("emits annotation changes when notes are edited", () => {
    const onChange = vi.fn();
    render(<SessionDetail session={mk()} onAnnotationChange={onChange} />);
    fireEvent.change(screen.getByPlaceholderText(/notes/i), { target: { value: "new note" } });
    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ notes: "new note" }),
    );
  });

  it("offers Resume for an idle session", () => {
    render(<SessionDetail session={mk({ liveness: "idle" })} onAnnotationChange={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /resume/i }));
    expect(resumeSession).toHaveBeenCalledWith("s1", "/code/repo");
  });

  it("offers Jump to terminal for a running session", () => {
    render(<SessionDetail session={mk({ liveness: "legacy" })} onAnnotationChange={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: /jump/i }));
    expect(focusSession).toHaveBeenCalledWith("/code/repo");
  });

  it("does not offer Jump for a session with no live process", () => {
    render(<SessionDetail session={mk({ liveness: "idle" })} onAnnotationChange={vi.fn()} />);
    expect(screen.queryByRole("button", { name: /jump/i })).toBeNull();
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `yarn vitest run src/components/SessionDetail.test.tsx`
Expected: FAIL — `./SessionDetail` does not exist.

- [ ] **Step 3: Implement StatusPicker**

Create `src/components/StatusPicker.tsx`:

```tsx
import type { ManualStatus } from "../types";

const OPTIONS: { value: ManualStatus; label: string }[] = [
  { value: "blocked", label: "Blocked" },
  { value: "needsReview", label: "Needs review" },
  { value: "waitingOnMe", label: "Waiting on me" },
  { value: "background", label: "Background" },
];

export function StatusPicker({
  value,
  onChange,
}: {
  value: ManualStatus | null;
  onChange: (v: ManualStatus | null) => void;
}) {
  return (
    <div className="flex flex-wrap gap-1">
      {OPTIONS.map((o) => (
        <button
          key={o.value}
          type="button"
          onClick={() => onChange(value === o.value ? null : o.value)}
          className={`rounded px-2 py-1 text-xs ${
            value === o.value
              ? "bg-purple-500/20 text-purple-300"
              : "bg-neutral-800 text-neutral-400 hover:text-neutral-200"
          }`}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
```

- [ ] **Step 4: Implement SessionDetail**

Create `src/components/SessionDetail.tsx`:

```tsx
import type { Annotation, Session } from "../types";
import { focusSession, resumeSession } from "../api/tauri";
import { StatusPicker } from "./StatusPicker";

export function SessionDetail({
  session,
  onAnnotationChange,
}: {
  session: Session | null;
  onAnnotationChange: (a: Annotation) => void;
}) {
  if (!session) {
    return (
      <div className="flex h-full items-center justify-center p-6 text-sm text-neutral-500">
        Select a session to see its details.
      </div>
    );
  }

  const a = session.annotation;
  const title = a.displayName ?? session.aiTitle ?? "Untitled session";
  const isRunning = session.liveness === "legacy" || session.liveness === "managed";

  return (
    <div className="flex h-full flex-col gap-4 overflow-y-auto p-5">
      <header>
        <h1 className="text-lg font-semibold text-neutral-100">{title}</h1>
        <p className="mt-0.5 text-xs text-neutral-500">
          {session.projectLabel}
          {session.gitBranch && <span className="ml-2 text-neutral-400">{session.gitBranch}</span>}
        </p>
      </header>

      <div className="flex gap-2">
        {isRunning && (
          <button
            type="button"
            onClick={() => void focusSession(session.cwd)}
            className="rounded bg-sky-500/15 px-3 py-1.5 text-sm text-sky-300 hover:bg-sky-500/25"
          >
            Jump to terminal
          </button>
        )}
        <button
          type="button"
          onClick={() => void resumeSession(session.sessionId, session.cwd)}
          className="rounded bg-neutral-800 px-3 py-1.5 text-sm text-neutral-200 hover:bg-neutral-700"
        >
          Resume in new tab
        </button>
      </div>

      <section>
        <h3 className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-neutral-500">
          Status
        </h3>
        <StatusPicker value={a.status} onChange={(status) => onAnnotationChange({ ...a, status })} />
      </section>

      <section className="flex min-h-0 flex-1 flex-col">
        <h3 className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-neutral-500">
          Notes
        </h3>
        <textarea
          value={a.notes}
          onChange={(e) => onAnnotationChange({ ...a, notes: e.target.value })}
          placeholder="Notes for this session…"
          className="min-h-[8rem] flex-1 resize-none rounded bg-neutral-800 p-2 text-sm text-neutral-100 outline-none placeholder:text-neutral-500 focus:ring-1 focus:ring-sky-500"
        />
      </section>

      <section>
        <h3 className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-neutral-500">
          Last prompt
        </h3>
        <p className="whitespace-pre-wrap break-words rounded bg-neutral-800/50 p-2 text-xs text-neutral-300">
          {session.lastPrompt ?? "—"}
        </p>
      </section>

      <footer className="text-[11px] text-neutral-600">
        <div className="break-all">{session.cwd}</div>
        <div>
          {session.sessionId}
          {session.version && ` · v${session.version}`}
        </div>
      </footer>
    </div>
  );
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `yarn vitest run src/components/SessionDetail.test.tsx`
Expected: 6 tests pass.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: add session detail pane with notes, status, and actions"
```

---

### Task 12: Wire the app together

**Files:**
- Modify: `src/App.tsx`, `src/main.tsx`
- Create: `src/App.test.tsx`

**Interfaces:**
- Consumes: everything from Tasks 9–11
- Produces: the running application. Polls `list_sessions` every 10 seconds via TanStack
  Query; annotation edits are debounced 500 ms before calling `set_annotation`.

**REQUIRED FIRST — an index cache. Measured, not hypothetical.**

A full `index_sessions` scan of the real tree takes **~10 seconds** (measured at 9.98s,
10.27s, 10.17s across three runs: 1312 depth-2 transcripts, 1.0 GiB). The original plan
polled every 3 seconds, so polls would overlap continuously and never settle.

Before wiring the UI, add an mtime-keyed cache to `src-tauri/src/index.rs` so a poll
re-parses only files that actually changed:

```rust
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Cache of parse results keyed by transcript path, with the mtime the parse was
/// made from. A poll re-parses a file only when its mtime moved.
///
/// The value is `Option<Session>`, NOT `Session`. This is the load-bearing detail:
/// only 37 of ~2887 transcripts survive the entrypoint filter, so caching just the
/// successes leaves ~2850 rejections to be re-parsed on every single poll. Measured,
/// that is a warm scan of ~2.5s instead of ~20ms -- a 125x difference. Cache the
/// rejections too.
///
/// `HashMap::new` is not const, so this cannot be a bare `static` initializer;
/// `LazyLock` is stable std and needs no new crate.
///
/// Freshness is full-precision mtime nanos plus file size -- NOT whole seconds.
/// An active session writes its transcript several times per second, so a
/// second-granularity key would serve a stale parse for exactly the live
/// sessions this dashboard exists to show. Size is a cheap second signal for
/// the rare same-nanosecond case. The DISPLAYED `last_activity` stays in whole
/// seconds; it is a human-facing timestamp, not a cache key.
type Freshness = (u128, u64);
type CacheEntry = (Freshness, Option<Session>);
static CACHE: LazyLock<Mutex<HashMap<PathBuf, CacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
```

Recover from a poisoned mutex rather than panicking — a poisoned cache must not take
down the app: `CACHE.lock().unwrap_or_else(|e| e.into_inner())`.

In `index_sessions`, for each `.jsonl` found: read its mtime, look the path up in the
cache, and reuse the cached `Session` when the mtime is unchanged. Otherwise parse and
store. Drop cache entries whose paths are no longer present so deleted sessions leave
the list. Keep `index_sessions(root)` signature-compatible — the cache is internal.

Add a test asserting a second scan of an unchanged fixture tree returns the same sessions,
and an `#[ignore]` test measuring that a second real-tree scan is dramatically faster than
the first (assert the second completes in under 2 seconds).

- [ ] **Step 1: Write the failing integration test**

Create `src/App.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import type { Session } from "./types";

const listSessions = vi.fn();
const setAnnotation = vi.fn();
vi.mock("./api/tauri", () => ({
  listSessions: () => listSessions(),
  setAnnotation: (...a: unknown[]) => setAnnotation(...a),
  focusSession: vi.fn(),
  resumeSession: vi.fn(),
}));

import App from "./App";

const mk = (over: Partial<Session> = {}): Session => ({
  sessionId: "s1",
  aiTitle: "Fix the parser",
  lastPrompt: "make it work",
  gitBranch: "main",
  cwd: "/code/repo",
  projectLabel: "repo",
  version: "2.1.220",
  lastActivity: 100,
  liveness: "idle",
  annotation: { notes: "", status: null, displayName: null },
  ...over,
});

describe("App", () => {
  beforeEach(() => {
    listSessions.mockReset();
    setAnnotation.mockReset();
  });

  it("renders sessions returned by the backend", async () => {
    listSessions.mockResolvedValue([mk()]);
    render(<App />);
    await waitFor(() => expect(screen.getByText("Fix the parser")).toBeDefined());
  });

  it("shows a count of loaded sessions", async () => {
    listSessions.mockResolvedValue([mk({ sessionId: "a" }), mk({ sessionId: "b" })]);
    render(<App />);
    await waitFor(() => expect(screen.getByText(/2 sessions/i)).toBeDefined());
  });

  it("surfaces an error state when the backend fails", async () => {
    listSessions.mockRejectedValue(new Error("boom"));
    render(<App />);
    await waitFor(() => expect(screen.getByText(/could not load sessions/i)).toBeDefined());
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `yarn vitest run src/App.test.tsx`
Expected: FAIL — App renders the template, not the session list.

- [ ] **Step 3: Implement App**

Replace `src/App.tsx` entirely:

```tsx
import { useEffect, useMemo, useRef, useState } from "react";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { listSessions, setAnnotation } from "./api/tauri";
import { applyFilters, useFilters } from "./store/filters";
import { FilterBar } from "./components/FilterBar";
import { SessionList } from "./components/SessionList";
import { SessionDetail } from "./components/SessionDetail";
import type { Annotation } from "./types";

const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

function Shell() {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const filters = useFilters();
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const { data, error, isLoading, refetch } = useQuery({
    queryKey: ["sessions"],
    queryFn: listSessions,
    // 10s, not 3s: a cold index scan of the real tree measured ~10s. With the
    // mtime cache a warm poll is far cheaper, but polling faster than a cold
    // scan risks overlapping requests on first launch.
    refetchInterval: 10000,
  });

  const sessions = data ?? [];
  const visible = useMemo(() => applyFilters(sessions, filters), [sessions, filters]);
  const selected = sessions.find((s) => s.sessionId === selectedId) ?? null;

  // Debounce annotation writes so typing does not hit the disk on every key.
  const [draft, setDraft] = useState<Annotation | null>(null);
  const shown = draft && selected ? { ...selected, annotation: draft } : selected;

  useEffect(() => setDraft(null), [selectedId]);

  function onAnnotationChange(a: Annotation) {
    if (!selected) return;
    setDraft(a);
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => {
      void setAnnotation(selected.sessionId, a).then(() => refetch());
    }, 500);
  }

  return (
    <div className="flex h-screen bg-neutral-900 text-neutral-100">
      <aside className="flex w-80 shrink-0 flex-col border-r border-neutral-800">
        <FilterBar />
        <div className="px-3 py-1 text-[11px] text-neutral-500">
          {isLoading ? "Loading…" : `${visible.length} sessions`}
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {error ? (
            <div className="p-4 text-sm text-amber-400">Could not load sessions.</div>
          ) : (
            <SessionList sessions={visible} selectedId={selectedId} onSelect={setSelectedId} />
          )}
        </div>
      </aside>
      <main className="min-w-0 flex-1">
        <SessionDetail session={shown} onAnnotationChange={onAnnotationChange} />
      </main>
    </div>
  );
}

export default function App() {
  return (
    <QueryClientProvider client={client}>
      <Shell />
    </QueryClientProvider>
  );
}
```

- [ ] **Step 4: Install and wire Tailwind**

The Tauri React scaffold does NOT ship Tailwind, despite every component from Task 10
onward being written with Tailwind utility classes. Without this step the app runs
completely unstyled. Install and wire it:

```bash
yarn add -D tailwindcss @tailwindcss/vite
```

In `vite.config.ts`, add the plugin:

```ts
import tailwindcss from "@tailwindcss/vite";
// ...
  plugins: [react(), tailwindcss()],
```

Set `src/index.css` to:

```css
@import "tailwindcss";
```

and confirm `src/main.tsx` imports it (`import "./index.css";`). Delete the scaffold's
`src/App.css` — it is residue and nothing imports it.

- [ ] **Step 5: Run tests to verify they pass**

Run: `yarn vitest run src/App.test.tsx`
Expected: 3 tests pass.

- [ ] **Step 6: Run the whole suite**

Run: `make test`
Expected: all Rust and frontend tests pass.

- [ ] **Step 7: Run the app against real data**

Run: `make dev`

Verify by hand:
1. The list populates with your real interactive sessions — a modest number (roughly 20), **not** hundreds. If you see hundreds, the entrypoint filter has regressed.
2. Worktree sessions display as `repo ▸ worktree`.
3. Typing in the search box narrows the list.
4. Selecting a session, typing a note, quitting the app, and relaunching shows the note still there.
5. A session with a live process shows "Running" and its "Jump to terminal" button focuses the right iTerm2 tab.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat: wire session list, detail, and polling into the app shell"
```

---

### Task 13: Verify success criteria

**Files:**
- Create: `docs/superpowers/plans/2026-07-30-phase-1-verification.md`

**Interfaces:**
- Consumes: the complete application
- Produces: a written record of each spec success criterion, measured rather than asserted.

- [ ] **Step 1: Measure criterion 1 — new sessions appear within 5s**

With `make dev` running, open a new terminal, `cd` to any repo, and run `claude`. Send one message so a transcript is written. Time how long until the session appears in Claudron.

Record the observed time. Polling is 3s, so expect under 5s.

- [ ] **Step 2: Measure criterion 2 — recover a worktree session in under 10s**

Pick a session from a worktree. Confirm it is **absent** from `claude --resume` when run from the parent repo:

```bash
cd $CLAUDRON_TEST_REPO && claude --resume
```

Then find the same session in Claudron and click Resume. Time from opening Claudron to a resumed session. Record it.

- [ ] **Step 3: Measure criterion 3 — annotations survive restarts**

Add a note and a status to a session. Quit Claudron entirely. Relaunch. Confirm both persist. Then verify the store on disk:

```bash
cat ~/.claudron/annotations.json
```

- [ ] **Step 4: Measure criterion 4 — memory under 500MB**

With the app running and the full session list loaded:

```bash
ps -eo pid,rss,comm | grep -i claudron
```

RSS is in KB; divide by 1024 for MB. Sum across the Claudron processes. Record the total.

Note: criterion 4's "3 terminals visible" clause does not apply in Phase 1 — there are no terminals yet. Record the list-only figure as the Phase 1 baseline.

- [ ] **Step 5: Write the verification record**

Create `docs/superpowers/plans/2026-07-30-phase-1-verification.md` with a table of the four criteria, the measured value for each, and PASS/FAIL. For any FAIL, note the observed value and the suspected cause.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "docs: record Phase 1 success criteria verification"
```

---

## Self-Review

**1. Spec coverage.** Every Phase 1 spec requirement maps to a task:

| Spec requirement | Task |
|---|---|
| Global index across all project dirs | 4 |
| Interactive-only (`entrypoint: cli`) filter | 2, 4 |
| Sidechain exclusion | 2 |
| Last-occurrence parsing of title/prompt | 2 |
| Auto-captured fields | 2, 4 |
| Worktree project labels | 3 |
| Liveness states (Legacy/Interrupted/Idle) | 4, 6, 8 |
| Notes + manual status, persisted | 5, 8, 11 |
| Annotation store keyed by sessionId | 5 |
| Jump to iTerm2 tab | 7, 8, 11 |
| Resume in correct cwd | 7, 8, 11 |
| Filtering and search | 10 |
| Malformed transcript never breaks indexing | 2 |
| Corrupt annotation store degrades gracefully | 5 |
| tmux absent must not break anything | Global constraints — no task touches tmux |
| Success criteria measured | 13 |

`Liveness::Managed` is defined in the model but unreachable in Phase 1 — intentional, so Phase 2 adds tmux without changing the type.

**2. Placeholder scan.** No TBDs, no "add error handling" instructions, no "similar to Task N" references. Every code step contains complete code.

**3. Type consistency.** `Session`, `Annotation`, `Liveness`, and `ManualStatus` field names match between `model.rs` (camelCase via serde) and `types.ts`. Command names match between `commands.rs` (`list_sessions`, `set_annotation`, `focus_session`, `resume_session`) and `api/tauri.ts`. `applyFilters` and `Filters` are consistent between `filters.ts` and its consumers in `App.tsx` and `FilterBar.tsx`. `parse_ps_output` is used only in `process.rs`. `assemble` is used only in `commands.rs`.
