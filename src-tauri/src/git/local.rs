use crate::git::model::{GitLocal, WorktreeInfo};
use crate::git::run::{run, RunError};
use std::path::Path;
use std::time::Duration;

const GIT_TIMEOUT: Duration = Duration::from_secs(5);

/// Parse `git rev-list --left-right --count HEAD...@{upstream}`.
///
/// Real output is TAB separated -- `0\t0` -- not spaces.
pub fn parse_ahead_behind(out: &str) -> Option<(u32, u32)> {
    let mut parts = out.split_whitespace();
    let a = parts.next()?.parse().ok()?;
    let b = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((a, b))
}

fn git(cwd: &Path, args: &[&str]) -> Result<String, RunError> {
    run("git", args, cwd, GIT_TIMEOUT)
}

/// Whether `branch` appears as a line in `git branch --merged <default>` output.
///
/// Each line carries a two-character prefix: `"* "` marks the branch checked
/// out in *this* worktree, `"+ "` marks a branch checked out in some *other*
/// worktree of the same repo (Claudron is worktree-centric, so this shows up
/// routinely). Stripping only `"* "` would silently misreport a merged branch
/// that happens to be checked out elsewhere as unmerged -- the one direction
/// this value must never be wrong in, since it gates a destructive
/// worktree-removal confirmation.
fn branch_list_contains(out: &str, branch: &str) -> bool {
    out.lines().any(|l| {
        let name = l.trim().trim_start_matches("* ").trim_start_matches("+ ");
        name == branch
    })
}

/// A failed `--git-common-dir` lookup must never be read as "this is a
/// worktree" -- that reading is what offers the destructive Remove button.
/// `common_dir_ok` is false exactly when that lookup failed, in which case
/// `repo_root` falls back to an empty string and would otherwise differ from
/// `toplevel` for every ordinary main checkout.
fn compute_is_worktree(common_dir_ok: bool, toplevel: &str, repo_root: &str) -> bool {
    common_dir_ok && !toplevel.is_empty() && toplevel != repo_root
}

pub fn inspect(cwd: &Path) -> Result<GitLocal, String> {
    // Any git command fails outside a repo; use the cheapest as the gate.
    let inside = git(cwd, &["rev-parse", "--is-inside-work-tree"])
        .map_err(|e| match e {
            RunError::Failed { stderr, .. } if !stderr.is_empty() => stderr,
            RunError::Timeout => "git timed out".to_string(),
            other => format!("{other:?}"),
        })?;
    if inside.trim() != "true" {
        return Err("not a git working tree".into());
    }

    let branch = git(cwd, &["branch", "--show-current"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let dirty_count = git(cwd, &["status", "--porcelain"])
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count() as u32)
        .unwrap_or(0);

    let (ahead, behind) = match git(cwd, &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"]) {
        Ok(out) => match parse_ahead_behind(&out) {
            Some((a, b)) => (Some(a), Some(b)),
            None => (None, None),
        },
        // No upstream configured -- distinct from being in sync.
        Err(_) => (None, None),
    };

    // ALWAYS --path-format=absolute: --git-common-dir alone returns a relative
    // ".git" from a main checkout and an absolute path from a worktree.
    let common_dir_ok = git(cwd, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).ok();
    let common = common_dir_ok
        .as_deref()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let repo_root = common
        .strip_suffix("/.git")
        .unwrap_or(common.trim_end_matches(".git").trim_end_matches('/'))
        .to_string();

    let toplevel = git(cwd, &["rev-parse", "--path-format=absolute", "--show-toplevel"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| cwd.to_string_lossy().to_string());

    let is_worktree = compute_is_worktree(common_dir_ok.is_some(), &toplevel, &repo_root);

    let default_branch = git(cwd, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .ok()
        .map(|s| s.trim().trim_start_matches("origin/").to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| Some("main".to_string()));

    let merged_into_default = match (&branch, &default_branch) {
        (Some(b), Some(d)) if b != d => git(cwd, &["branch", "--merged", d])
            .ok()
            .map(|out| branch_list_contains(&out, b)),
        _ => None,
    };

    Ok(GitLocal {
        branch,
        dirty_count,
        ahead,
        behind,
        worktree: WorktreeInfo { is_worktree, path: toplevel, repo_root },
        default_branch,
        merged_into_default,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tab_separated_counts() {
        // Measured real output: "0\t0".
        assert_eq!(parse_ahead_behind("0\t0\n"), Some((0, 0)));
        assert_eq!(parse_ahead_behind("3\t7"), Some((3, 7)));
    }

    #[test]
    fn rejects_output_that_is_not_two_counts() {
        assert_eq!(parse_ahead_behind(""), None);
        assert_eq!(parse_ahead_behind("fatal: no upstream"), None);
        assert_eq!(parse_ahead_behind("1"), None);
    }

    /// Build a real git repo in a temp dir. Subprocess behaviour is what these
    /// tests are for, so they use actual git rather than mocking it.
    fn init_repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            run("git", &args, p, GIT_TIMEOUT).unwrap();
        }
        std::fs::write(p.join("a.txt"), b"hello").unwrap();
        run("git", &["add", "."], p, GIT_TIMEOUT).unwrap();
        run("git", &["commit", "-q", "-m", "first"], p, GIT_TIMEOUT).unwrap();
        d
    }

    #[test]
    fn reports_a_clean_repo_on_its_branch() {
        let d = init_repo();
        let g = inspect(d.path()).unwrap();
        assert_eq!(g.branch.as_deref(), Some("main"));
        assert_eq!(g.dirty_count, 0);
    }

    #[test]
    fn counts_changed_paths_in_a_dirty_repo() {
        let d = init_repo();
        std::fs::write(d.path().join("a.txt"), b"changed").unwrap();
        std::fs::write(d.path().join("b.txt"), b"new").unwrap();
        let g = inspect(d.path()).unwrap();
        assert_eq!(g.dirty_count, 2, "one modified plus one untracked");
    }

    #[test]
    fn no_upstream_yields_none_not_zero() {
        // Some(0) means "in sync"; None means "there is no upstream at all".
        // Rendering them the same would tell the user they are up to date when
        // nothing is being tracked.
        let d = init_repo();
        let g = inspect(d.path()).unwrap();
        assert_eq!(g.ahead, None);
        assert_eq!(g.behind, None);
    }

    #[test]
    fn a_main_checkout_is_not_a_worktree_but_resolves_its_repo_root() {
        let d = init_repo();
        let g = inspect(d.path()).unwrap();
        assert!(!g.worktree.is_worktree);
        // --git-common-dir is RELATIVE from a main checkout, so resolution must
        // pass --path-format=absolute or repo_root ends up as ".git". Assert
        // exact equality against the canonicalized fixture dir, not just
        // "looks absolute" -- macOS TempDir paths go through the /private
        // symlink, so canonicalize both sides or they will never compare equal.
        let expected = d.path().canonicalize().unwrap();
        assert_eq!(
            std::path::Path::new(&g.worktree.repo_root).canonicalize().unwrap(),
            expected,
            "repo_root must be the main checkout itself, got {:?}",
            g.worktree.repo_root
        );
        assert_eq!(
            std::path::Path::new(&g.worktree.path).canonicalize().unwrap(),
            expected,
            "path must be the main checkout itself, got {:?}",
            g.worktree.path
        );
        assert!(!g.worktree.repo_root.ends_with(".git"));
    }

    #[test]
    fn a_linked_worktree_is_detected_and_points_at_its_parent() {
        let d = init_repo();
        let wt = d.path().join("wt");
        run(
            "git",
            &["worktree", "add", "-q", "-b", "feature", wt.to_str().unwrap()],
            d.path(),
            GIT_TIMEOUT,
        )
        .unwrap();

        let g = inspect(&wt).unwrap();
        assert!(g.worktree.is_worktree, "a linked worktree must be detected");
        assert_eq!(g.branch.as_deref(), Some("feature"));
        // Exact equality, not just "looks absolute": repo_root must be the
        // PARENT repo, and path must be the worktree itself -- distinct
        // directories. A bug that conflated the two would previously slip
        // past a starts_with('/') check.
        assert_eq!(
            std::path::Path::new(&g.worktree.repo_root).canonicalize().unwrap(),
            d.path().canonicalize().unwrap(),
            "repo_root must be the parent repo, got {:?}",
            g.worktree.repo_root
        );
        assert_eq!(
            std::path::Path::new(&g.worktree.path).canonicalize().unwrap(),
            wt.canonicalize().unwrap(),
            "path must be the worktree itself, got {:?}",
            g.worktree.path
        );
    }

    #[test]
    fn falls_back_to_main_when_origin_head_is_unset() {
        // Measured: most repos resolve origin/HEAD, but some (including this one)
        // itself does NOT. A resolver without a fallback errors on a real repo.
        let d = init_repo();
        let g = inspect(d.path()).unwrap();
        assert_eq!(g.default_branch.as_deref(), Some("main"));
    }

    #[test]
    fn a_directory_that_is_not_a_repo_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        assert!(inspect(d.path()).is_err());
    }

    /// Create a feature branch off `main` (the default) with one commit on it,
    /// leaving HEAD checked out on the feature branch. `init_repo()` already
    /// pins the default branch name explicitly via `-b main`, so this is not
    /// dependent on the machine's `init.defaultBranch`.
    fn branch_off_default(d: &tempfile::TempDir, name: &str) {
        run("git", &["checkout", "-q", "-b", name], d.path(), GIT_TIMEOUT).unwrap();
        std::fs::write(d.path().join("feature.txt"), b"work").unwrap();
        run("git", &["add", "."], d.path(), GIT_TIMEOUT).unwrap();
        run("git", &["commit", "-q", "-m", "feature work"], d.path(), GIT_TIMEOUT).unwrap();
    }

    #[test]
    fn a_branch_merged_into_default_reports_true() {
        let d = init_repo();
        branch_off_default(&d, "feature");
        // Merge feature into main, then come back to feature -- it is now
        // fully contained in main's history.
        run("git", &["checkout", "-q", "main"], d.path(), GIT_TIMEOUT).unwrap();
        run("git", &["merge", "-q", "feature"], d.path(), GIT_TIMEOUT).unwrap();
        run("git", &["checkout", "-q", "feature"], d.path(), GIT_TIMEOUT).unwrap();

        let g = inspect(d.path()).unwrap();
        assert_eq!(g.branch.as_deref(), Some("feature"));
        assert_eq!(g.default_branch.as_deref(), Some("main"));
        assert_eq!(g.merged_into_default, Some(true));
    }

    #[test]
    fn a_branch_not_merged_into_default_reports_false() {
        let d = init_repo();
        branch_off_default(&d, "feature");
        // No merge back into main -- feature's commit is not in main's history.

        let g = inspect(d.path()).unwrap();
        assert_eq!(g.branch.as_deref(), Some("feature"));
        assert_eq!(g.default_branch.as_deref(), Some("main"));
        assert_eq!(
            g.merged_into_default,
            Some(false),
            "must be Some(false), not None -- an unmerged branch is a determined answer"
        );
    }

    #[test]
    fn detached_head_cannot_determine_merge_status_and_reports_none() {
        // A genuinely distinct reason for None from the other tests: here
        // there IS a real default branch and a real answerable question, but
        // `branch --show-current` prints nothing in detached HEAD, so there is
        // no branch name to ask "is this merged" about. The determination
        // genuinely cannot be made -- this must never be presented as "not
        // merged" (Some(false)), only as unknown.
        let d = init_repo();
        branch_off_default(&d, "feature");
        run("git", &["checkout", "-q", "--detach"], d.path(), GIT_TIMEOUT).unwrap();

        let g = inspect(d.path()).unwrap();
        assert_eq!(g.branch, None, "detached HEAD has no branch name");
        assert_eq!(g.merged_into_default, None);
    }

    #[test]
    fn branch_list_recognizes_a_branch_marked_plus_from_another_worktree() {
        // Real `git branch --merged <default>` output, reproduced by hand:
        // running the command from a main checkout where `feature` (merged
        // into main) is checked out in a SEPARATE linked worktree marks it
        // "+ feature", not "* feature" -- "*" is reserved for whichever
        // branch is checked out in the directory the command was run from.
        //
        // This cannot be provoked through the public inspect() entry point:
        // `b` there always comes from --show-current on `cwd` itself, and
        // git always marks that branch "* ", never "+ ", from that same cwd.
        // So this is tested directly against the pure line-matching helper
        // instead of faking it through a worktree inspect() would never
        // actually see this combination through.
        let out = "+ feature\n* main\n";
        assert!(
            branch_list_contains(out, "feature"),
            "a branch checked out in another worktree must still count as merged"
        );
        assert!(branch_list_contains(out, "main"));
        assert!(!branch_list_contains(out, "something-else"));
    }

    #[test]
    fn compute_is_worktree_treats_a_failed_common_dir_lookup_as_not_a_worktree() {
        // A failed --git-common-dir call must never be read as "this is a
        // worktree": that reading is what offers the destructive Remove
        // button. Before the fix, a failure fell back to repo_root == "",
        // which differs from any non-empty toplevel and so read as true.
        assert!(
            !compute_is_worktree(false, "/some/checkout", ""),
            "a failed lookup must yield false regardless of what the paths look like"
        );
        // Sanity: the ordinary cases still behave once the lookup succeeds.
        assert!(!compute_is_worktree(true, "/repo", "/repo"), "same path is a main checkout");
        assert!(compute_is_worktree(true, "/repo/wt", "/repo"), "differing paths is a worktree");
    }

    /// Times `inspect` against a real checkout, to catch costs that only appear
    /// at scale -- `git worktree list` is the expensive call, and a repo with
    /// dozens of worktrees is where the 200 ms budget is actually tested.
    ///
    /// Point `CLAUDRON_TEST_REPO` at any real git repository to run it:
    ///   CLAUDRON_TEST_REPO=~/code/some-repo cargo test inspects_a_real -- --ignored --nocapture
    #[test]
    #[ignore]
    fn inspects_a_real_checkout() {
        let Ok(dir) = std::env::var("CLAUDRON_TEST_REPO") else {
            println!("set CLAUDRON_TEST_REPO to a real git repo to run this; skipping");
            return;
        };
        let p = std::path::PathBuf::from(dir);
        if !p.exists() {
            println!("CLAUDRON_TEST_REPO does not exist; skipping");
            return;
        }
        let t0 = std::time::Instant::now();
        let g = inspect(&p).unwrap();
        let elapsed = t0.elapsed();
        println!(
            "branch={:?} dirty={} ahead={:?} behind={:?} worktree={} root={:?} default={:?} in {elapsed:?}",
            g.branch, g.dirty_count, g.ahead, g.behind, g.worktree.is_worktree,
            g.worktree.repo_root, g.default_branch
        );
        assert!(
            elapsed < std::time::Duration::from_millis(200),
            "criterion 1 requires under 200ms, took {elapsed:?}"
        );
    }
}
