pub mod local;
pub mod model;
pub mod remote;
pub mod run;

use model::{GitLocal, GitRemote};
use std::path::Path;
use std::time::Duration;

#[tauri::command]
pub fn git_local(cwd: String) -> Result<GitLocal, String> {
    local::inspect(Path::new(&cwd))
}

#[tauri::command]
pub fn git_remote(cwd: String, branch: String) -> Result<GitRemote, String> {
    remote::fetch(Path::new(&cwd), &branch)
}

/// Remove a worktree, refusing when it is dirty, not a worktree, or in use.
///
/// Uses `git worktree remove` and never `rm -rf`: git applies its own refusal
/// conditions beneath ours, and the branch is deliberately left alone. A branch
/// is easy to recover; uncommitted work is not.
#[tauri::command]
pub fn remove_worktree(path: String) -> Result<(), String> {
    let live_cwds: Vec<String> = crate::process::discover_claude_processes()
        .into_iter()
        .filter_map(|p| p.cwd)
        .collect();
    remove_worktree_impl(&path, &live_cwds)
}

/// The guarded removal logic, taking `live_cwds` as a parameter rather than
/// discovering it itself.
///
/// Split out so the third safety guard -- refusing removal while a live
/// process occupies the worktree -- can be proven with a fabricated cwd list
/// instead of a real, timing-sensitive `claude` subprocess plus `ps`/`lsof`
/// polling. The public `#[tauri::command]` is a thin wrapper that supplies
/// the real list from `crate::process::discover_claude_processes`; this
/// function carries every actual guard and is what the tests exercise.
fn remove_worktree_impl(path: &str, live_cwds: &[String]) -> Result<(), String> {
    let p = Path::new(path);

    let g = local::inspect(p)?;
    if !g.worktree.is_worktree {
        return Err("not a worktree -- refusing to remove a main checkout".into());
    }
    if g.dirty_count > 0 {
        return Err(format!(
            "worktree has {} uncommitted change(s) -- commit or discard them first",
            g.dirty_count
        ));
    }
    // Compare CANONICALIZED forms, not raw strings. `live_cwds` comes from
    // `lsof -d cwd -Fn` (process.rs), which reports the fully resolved path;
    // `path` here is whatever the caller supplied, never canonicalized. On
    // macOS a plain tempdir already aliases through /private (see local.rs's
    // own note on exactly this), and /var, /etc, and any user symlink behave
    // the same -- so a raw string comparison silently fails to match the
    // same real directory and lets removal proceed with a live process still
    // sitting in it. `local::inspect` has already succeeded by this point,
    // so `path` is known to exist and canonicalize. A live cwd that fails to
    // canonicalize (e.g. it names a directory that no longer exists) falls
    // back to its raw form rather than being dropped from consideration --
    // dropping it would silently widen the set of paths this guard ignores.
    //
    // A live cwd must block removal when it EQUALS the worktree OR sits
    // ANYWHERE UNDER it, not just on exact equality. Confirmed against real
    // git 2.50.1: `git worktree remove` happily deletes a worktree with a
    // process cwd'd into a subdirectory of it -- exit 0, directory gone --
    // even when git itself is invoked from outside the worktree. There is no
    // second net beneath this guard for that case, unlike the other two.
    // `Path::starts_with` is used rather than a string-prefix check because
    // it compares path COMPONENTS: a raw `str::starts_with` would wrongly
    // treat a sibling directory like `/repo/wt-backup` as "under" `/repo/wt`
    // purely because one string happens to prefix the other.
    let canonical_path = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let occupied = live_cwds.iter().any(|c| {
        let canonical_c = std::fs::canonicalize(c).unwrap_or_else(|_| Path::new(c).to_path_buf());
        canonical_c.starts_with(&canonical_path)
    });
    if occupied {
        return Err(
            "a live session is running in this worktree -- stop it before removing".into(),
        );
    }

    // Run from the parent repo: removing a worktree from inside itself fails.
    run::run(
        "git",
        &["worktree", "remove", path],
        Path::new(&g.worktree.repo_root),
        Duration::from_secs(5),
    )
    .map(|_| ())
    .map_err(|e| match e {
        run::RunError::Failed { stderr, .. } if !stderr.is_empty() => stderr,
        run::RunError::Timeout => "git timed out".to_string(),
        other => format!("{other:?}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::run::run as sh;

    fn init_repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        let t = Duration::from_secs(5);
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            sh("git", &args, p, t).unwrap();
        }
        std::fs::write(p.join("a.txt"), b"hello").unwrap();
        sh("git", &["add", "."], p, t).unwrap();
        sh("git", &["commit", "-q", "-m", "first"], p, t).unwrap();
        d
    }

    fn add_worktree(repo: &Path, name: &str) -> std::path::PathBuf {
        let wt = repo.join(name);
        sh(
            "git",
            &["worktree", "add", "-q", "-b", name, wt.to_str().unwrap()],
            repo,
            Duration::from_secs(5),
        )
        .unwrap();
        wt
    }

    #[test]
    fn removes_a_clean_worktree() {
        let d = init_repo();
        let wt = add_worktree(d.path(), "feature");
        assert!(wt.exists());
        remove_worktree_impl(&wt.to_string_lossy(), &[]).unwrap();
        assert!(!wt.exists(), "the worktree directory must be gone");
    }

    #[test]
    fn refuses_to_remove_a_dirty_worktree() {
        let d = init_repo();
        let wt = add_worktree(d.path(), "dirty");
        std::fs::write(wt.join("scratch.txt"), b"uncommitted").unwrap();

        let e = remove_worktree_impl(&wt.to_string_lossy(), &[]).unwrap_err();
        assert!(e.contains("uncommitted"), "got {e:?}");
        assert!(wt.exists(), "a refused removal must leave the worktree in place");
    }

    #[test]
    fn refuses_to_remove_a_main_checkout() {
        let d = init_repo();
        let e = remove_worktree_impl(&d.path().to_string_lossy(), &[]).unwrap_err();
        assert!(e.contains("not a worktree"), "got {e:?}");
        assert!(d.path().exists());
    }

    #[test]
    fn leaves_the_branch_behind_after_removing_its_worktree() {
        // Removing a worktree must not delete work. The branch stays.
        let d = init_repo();
        let wt = add_worktree(d.path(), "keepme");
        remove_worktree_impl(&wt.to_string_lossy(), &[]).unwrap();

        let branches = sh("git", &["branch"], d.path(), Duration::from_secs(5)).unwrap();
        assert!(branches.contains("keepme"), "branch was deleted: {branches:?}");
    }

    #[test]
    fn refuses_to_remove_a_worktree_occupied_by_a_live_process() {
        // The third safety guard: a live process's cwd exactly matches the
        // worktree being removed. Fabricated live_cwds rather than a real
        // `claude` subprocess -- deterministic, and does not depend on the
        // `claude` CLI being on PATH.
        let d = init_repo();
        let wt = add_worktree(d.path(), "occupied");
        let wt_str = wt.to_string_lossy().to_string();

        let e = remove_worktree_impl(&wt_str, std::slice::from_ref(&wt_str)).unwrap_err();
        assert!(e.contains("live session"), "got {e:?}");
        assert!(wt.exists(), "a refused removal must leave the worktree in place");
    }

    #[test]
    fn refuses_to_remove_when_the_live_cwd_is_a_path_alias_of_the_same_directory() {
        // Regression for the exact-string-match bypass: lsof (process.rs)
        // reports a FULLY RESOLVED cwd, while `path` here comes from
        // whatever the caller passed in unresolved -- on macOS, tempdir()
        // paths go through the /private symlink (see local.rs's own comment
        // on this), so a raw string comparison between the two never
        // matches even though they name the identical directory. Unlike
        // `refuses_to_remove_a_worktree_occupied_by_a_live_process`, which
        // clones the SAME string for both sides and so cannot exercise this
        // divergence, this test builds the two sides from genuinely
        // different, independently-derived forms of the same path.
        let d = init_repo();
        let wt = add_worktree(d.path(), "aliased");

        // The uncanonicalized form, as a caller (e.g. a session's recorded
        // cwd) might supply it.
        let path = wt.to_string_lossy().to_string();
        // The canonicalized form, as lsof -Fn actually reports it.
        let canonical_live_cwd = wt.canonicalize().unwrap().to_string_lossy().to_string();
        assert_ne!(
            path, canonical_live_cwd,
            "fixture did not actually exercise aliasing on this machine -- \
             tempdir() must resolve through a symlink for this test to mean anything"
        );

        let e = remove_worktree_impl(&path, &[canonical_live_cwd]).unwrap_err();
        assert!(e.contains("live session"), "got {e:?}");
        assert!(wt.exists(), "a refused removal must leave the worktree in place");
    }

    #[test]
    fn refuses_to_remove_when_a_live_process_is_in_an_immediate_subdirectory() {
        // Regression for the missing-second-net bug: confirmed against real
        // git 2.50.1 that `git worktree remove` deletes a worktree with exit
        // 0 even while a process's cwd sits one directory below the
        // worktree root. Exact-equality matching (the prior fix) does not
        // catch this -- only containment does.
        let d = init_repo();
        let wt = add_worktree(d.path(), "sub-occupied");
        let sub = wt.join("sub");
        std::fs::create_dir(&sub).unwrap();
        let live_cwd = sub.canonicalize().unwrap().to_string_lossy().to_string();

        let e = remove_worktree_impl(&wt.to_string_lossy(), &[live_cwd]).unwrap_err();
        assert!(e.contains("live session"), "got {e:?}");
        assert!(wt.exists(), "a refused removal must leave the worktree in place");
    }

    #[test]
    fn refuses_to_remove_when_a_live_process_is_deeply_nested() {
        // Same as the immediate-subdirectory case, but several levels down --
        // containment must not be limited to a single path segment.
        let d = init_repo();
        let wt = add_worktree(d.path(), "deep-occupied");
        let deep = wt.join("a").join("b").join("c");
        std::fs::create_dir_all(&deep).unwrap();
        let live_cwd = deep.canonicalize().unwrap().to_string_lossy().to_string();

        let e = remove_worktree_impl(&wt.to_string_lossy(), &[live_cwd]).unwrap_err();
        assert!(e.contains("live session"), "got {e:?}");
        assert!(wt.exists(), "a refused removal must leave the worktree in place");
    }

    #[test]
    fn a_sibling_directory_that_shares_a_string_prefix_does_not_block_removal() {
        // Proves the containment check is component-wise (Path::starts_with),
        // not a naive string prefix: "/repo/wt-backup" textually starts with
        // "/repo/wt" but is a completely different, sibling directory and
        // must not be treated as occupying the worktree being removed.
        let d = init_repo();
        let wt = add_worktree(d.path(), "wt");
        let sibling = d.path().join("wt-backup");
        std::fs::create_dir(&sibling).unwrap();
        let live_cwd = sibling.canonicalize().unwrap().to_string_lossy().to_string();

        remove_worktree_impl(&wt.to_string_lossy(), &[live_cwd]).unwrap();
        assert!(!wt.exists(), "a same-prefix sibling must not block removal");
    }

    #[test]
    fn a_live_process_elsewhere_does_not_block_removal() {
        // A live process's cwd that is merely nearby (parent dir, sibling
        // worktree) must not block removal of a path it does not occupy.
        let d = init_repo();
        let wt = add_worktree(d.path(), "unrelated-occupant");
        let other = add_worktree(d.path(), "elsewhere");

        remove_worktree_impl(
            &wt.to_string_lossy(),
            &[
                d.path().to_string_lossy().to_string(),
                other.to_string_lossy().to_string(),
            ],
        )
        .unwrap();
        assert!(!wt.exists());
    }

    #[test]
    fn dirtiness_is_checked_before_liveness_so_the_message_names_the_real_blocker() {
        // Both guards could independently refuse; dirty must win so the user
        // is told to commit/discard rather than being told to stop a session
        // that, once stopped, would still leave the removal refused anyway.
        let d = init_repo();
        let wt = add_worktree(d.path(), "dirty-and-live");
        std::fs::write(wt.join("scratch.txt"), b"uncommitted").unwrap();
        let wt_str = wt.to_string_lossy().to_string();

        let e = remove_worktree_impl(&wt_str, std::slice::from_ref(&wt_str)).unwrap_err();
        assert!(e.contains("uncommitted"), "got {e:?}");
    }
}
