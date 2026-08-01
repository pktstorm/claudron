/// Turn an absolute cwd into a short two-part display label.
///
/// Worktrees live under a `worktrees/` or `.claude-worktrees/` directory
/// inside the parent repo, so `/x/api-service/worktrees/foo` renders as
/// `api-service ▸ foo` rather than the unreadable truncation an iTerm2 tab
/// title would give.
pub fn project_label(cwd: &str) -> String {
    let trimmed = cwd.trim_end_matches('/');
    if trimmed.is_empty() {
        return if cwd.starts_with('/') {
            "/".into()
        } else {
            "unknown".into()
        };
    }

    let parts: Vec<&str> = trimmed.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return "/".into();
    }

    let tree_name = parts[parts.len() - 1];

    // Case 1: .claude/worktrees/<name> layout
    // Need at least 4 parts: [..., ".claude", "worktrees", <name>]
    if parts.len() >= 4 {
        let marker = parts[parts.len() - 2];
        let parent = parts[parts.len() - 3];
        if marker == "worktrees" && parent == ".claude" {
            // The repo is at parts[len-4]
            if let Some(&repo) = parts.get(parts.len() - 4) {
                return format!("{} ▸ {}", repo, tree_name);
            }
            // Short path like /a/.claude/worktrees/b, fall through to plain name
        }
    }

    // Case 2: <repo>/.worktrees/<name> or <repo>/worktrees/<name> or <repo>/.claude-worktrees/<name>
    // Need at least 3 parts: [..., <repo>, marker, <name>]
    if parts.len() >= 3 {
        let marker = parts[parts.len() - 2];
        if marker == "worktrees" || marker == ".worktrees" || marker == ".claude-worktrees" {
            // The repo is at parts[len-3]
            if let Some(&repo) = parts.get(parts.len() - 3) {
                return format!("{} ▸ {}", repo, tree_name);
            }
            // Short path like /worktrees/b, fall through to plain name
        }
    }

    // Case 3: Plain directory name
    tree_name.to_string()
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

    #[test]
    fn trailing_slash_is_ignored() {
        assert_eq!(project_label("/Users/s/code/api-service/"), "api-service");
    }

    #[test]
    fn root_and_empty_degrade_gracefully() {
        assert_eq!(project_label("/"), "/");
        assert_eq!(project_label(""), "unknown");
    }

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
        let label = project_label("/Users/sthirlwall/code/admin-console/.claude/worktrees/agent-x");
        assert!(!label.starts_with(".claude"), "got {label}");
        assert_eq!(label, "admin-console ▸ agent-x");
    }

    #[test]
    fn short_worktree_paths_do_not_panic() {
        assert_eq!(project_label("/worktrees/b"), "b");
        assert_eq!(project_label("/a/.worktrees/b"), "a ▸ b");
    }
}
