use serde::{Deserialize, Serialize};

/// Whether the session's directory is a git worktree, and where its repo lives.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    /// True when this directory is a linked worktree rather than a main checkout.
    pub is_worktree: bool,
    /// Absolute path of the worktree itself.
    pub path: String,
    /// Absolute path of the repository this worktree belongs to.
    pub repo_root: String,
}

/// Everything derivable from local git, with no network access.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLocal {
    pub branch: Option<String>,
    /// Number of changed paths. 0 means clean.
    pub dirty_count: u32,
    /// Commits ahead of upstream. None when there is no upstream at all --
    /// distinct from Some(0), which means "in sync".
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub worktree: WorktreeInfo,
    /// Default branch, resolved from origin/HEAD or falling back to `main`.
    pub default_branch: Option<String>,
    /// Whether `branch` is merged into `default_branch`. None when it could not
    /// be determined -- which must render as unknown, never as "not merged".
    pub merged_into_default: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CheckState {
    Passing,
    Failing,
    Pending,
    /// No checks reported at all.
    None,
}

/// Rolled-up CI state for a pull request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckRollup {
    pub state: CheckState,
    pub passing: u32,
    pub failing: u32,
    pub pending: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub checks: CheckRollup,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRemote {
    /// None when the branch has no open pull request.
    pub pull_request: Option<PullRequest>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worktree() -> WorktreeInfo {
        WorktreeInfo {
            is_worktree: true,
            path: "/repo/.claude/worktrees/x".into(),
            repo_root: "/repo".into(),
        }
    }

    #[test]
    fn git_local_serializes_to_camel_case() {
        let l = GitLocal {
            branch: Some("main".into()),
            dirty_count: 3,
            ahead: Some(1),
            behind: Some(0),
            worktree: worktree(),
            default_branch: Some("main".into()),
            merged_into_default: Some(false),
        };
        let j = serde_json::to_string(&l).unwrap();
        assert!(j.contains("\"dirtyCount\":3"), "got {j}");
        assert!(j.contains("\"isWorktree\":true"), "got {j}");
        assert!(j.contains("\"repoRoot\":\"/repo\""), "got {j}");
        assert!(j.contains("\"mergedIntoDefault\":false"), "got {j}");
    }

    #[test]
    fn absent_optionals_are_explicit_null_not_omitted() {
        // The TS side types these as `T | null`; omitting them would break it.
        let l = GitLocal {
            branch: None,
            dirty_count: 0,
            ahead: None,
            behind: None,
            worktree: worktree(),
            default_branch: None,
            merged_into_default: None,
        };
        let j = serde_json::to_string(&l).unwrap();
        assert!(j.contains("\"ahead\":null"), "got {j}");
        assert!(j.contains("\"mergedIntoDefault\":null"), "got {j}");
    }

    #[test]
    fn check_state_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&CheckState::Passing).unwrap(), "\"passing\"");
        assert_eq!(serde_json::to_string(&CheckState::None).unwrap(), "\"none\"");
    }

    #[test]
    fn git_remote_with_no_pull_request_is_explicit_null() {
        let r = GitRemote { pull_request: None };
        assert_eq!(serde_json::to_string(&r).unwrap(), "{\"pullRequest\":null}");
    }
}
