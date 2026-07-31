use crate::git::model::{CheckRollup, CheckState, GitRemote, PullRequest};
use crate::git::run::{run, RunError};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

/// `gh` reaches the network -- measured at ~835 ms against ~30 ms for local git.
const GH_TIMEOUT: Duration = Duration::from_secs(15);

/// Classify one check. Anything not clearly finished-and-bad is not a failure.
///
/// `statusCheckRollup` entries are a union of two GitHub GraphQL types:
/// - `CheckRun` (GitHub Actions): has `status` + `conclusion`.
/// - `StatusContext` (external CI -- CircleCI, Jenkins, Codecov, Vercel, etc.):
///   has neither; instead it reports `state` directly, one of EXPECTED, ERROR,
///   FAILURE, PENDING, SUCCESS. A `StatusContext` has no `conclusion`/`status`
///   fields at all, so without a fallback it silently reads as Pending
///   regardless of its real state -- inverting "failing must win" for the
///   exact case (a red external CI check) that rule exists to catch.
///
/// Real conclusions seen: SUCCESS, SKIPPED, FAILURE, and "" while QUEUED.
/// Full `CheckConclusionState` enum: ACTION_REQUIRED, TIMED_OUT, CANCELLED,
/// FAILURE, SUCCESS, NEUTRAL, SKIPPED, STARTUP_FAILURE, STALE. STARTUP_FAILURE
/// (a workflow that failed to start) is unambiguously red. STALE is genuinely
/// ambiguous and is left to the Pending default.
fn classify(conclusion: &str, status: &str, state: &str) -> CheckState {
    match conclusion.to_ascii_uppercase().as_str() {
        "SUCCESS" | "SKIPPED" | "NEUTRAL" => return CheckState::Passing,
        "FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE" => {
            return CheckState::Failing;
        }
        "" if status.eq_ignore_ascii_case("COMPLETED") => return CheckState::Passing,
        _ => {}
    }

    // `state` (StatusContext's field) is consulted only when a genuine
    // CheckRun would have nothing left to say either -- conclusion AND
    // status both empty. A CheckRun in flight always populates `status`
    // (QUEUED, IN_PROGRESS, ...) even before it has a conclusion, so gating
    // on status.is_empty() too keeps an in-flight CheckRun with a stray
    // `state` field reading as Pending rather than picking up that field.
    if conclusion.is_empty() && status.is_empty() {
        return match state.to_ascii_uppercase().as_str() {
            "FAILURE" | "ERROR" => CheckState::Failing,
            "SUCCESS" => CheckState::Passing,
            _ => CheckState::Pending,
        };
    }

    CheckState::Pending
}

/// Parse the first entry of `gh pr list --json number,title,state,statusCheckRollup`.
pub fn parse_pr_json(json: &str) -> Option<PullRequest> {
    let v: Value = serde_json::from_str(json).ok()?;
    let first = v.as_array()?.first()?;

    let mut passing = 0;
    let mut failing = 0;
    let mut pending = 0;
    if let Some(checks) = first.get("statusCheckRollup").and_then(Value::as_array) {
        for c in checks {
            let conclusion = c.get("conclusion").and_then(Value::as_str).unwrap_or("");
            let status = c.get("status").and_then(Value::as_str).unwrap_or("");
            let state = c.get("state").and_then(Value::as_str).unwrap_or("");
            match classify(conclusion, status, state) {
                CheckState::Passing => passing += 1,
                CheckState::Failing => failing += 1,
                _ => pending += 1,
            }
        }
    }

    // Failing outranks pending outranks passing: the worst news is the headline.
    let state = if failing > 0 {
        CheckState::Failing
    } else if pending > 0 {
        CheckState::Pending
    } else if passing > 0 {
        CheckState::Passing
    } else {
        CheckState::None
    };

    Some(PullRequest {
        number: first.get("number").and_then(Value::as_u64)?,
        title: first.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
        state: first.get("state").and_then(Value::as_str).unwrap_or("").to_string(),
        checks: CheckRollup { state, passing, failing, pending },
    })
}

/// Map a subprocess failure to a message the UI can show. A pure function of
/// `RunError`, independent of how the error was produced -- tested directly
/// against hand-built `RunError` values rather than by spawning a process, so
/// no test needs to touch PATH or any other process-global state.
fn describe_fetch_error(e: RunError) -> String {
    match e {
        RunError::Spawn(_) => "gh is not installed or not on PATH".to_string(),
        RunError::Timeout => "gh timed out".to_string(),
        // Exit code 4 is gh's dedicated, documented auth-error code -- the
        // primary signal. `stderr.contains("auth")` only matched incidentally
        // (via the words "gh auth login" in the message text), which is not
        // something gh commits to; kept only as a secondary fallback in case
        // a future gh version changes the message but not the code.
        RunError::Failed { code: Some(4), .. } => {
            "gh is not authenticated -- run `gh auth login`".to_string()
        }
        RunError::Failed { stderr, .. } if stderr.contains("auth") => {
            "gh is not authenticated -- run `gh auth login`".to_string()
        }
        RunError::Failed { stderr, .. } if !stderr.is_empty() => stderr,
        other => format!("{other:?}"),
    }
}

pub fn fetch(cwd: &Path, branch: &str) -> Result<GitRemote, String> {
    let out = run(
        "gh",
        &[
            "pr", "list",
            "--head", branch,
            "--limit", "1",
            "--json", "number,title,state,statusCheckRollup",
        ],
        cwd,
        GH_TIMEOUT,
    )
    .map_err(describe_fetch_error)?;

    Ok(GitRemote { pull_request: parse_pr_json(&out) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PATH and gh's auth env vars are process-global; serialize tests that
    /// mutate them so parallel test threads do not stomp on each other or on
    /// unrelated tests that shell out to `git`/`gh`.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::LazyLock<std::sync::Mutex<()>> =
            std::sync::LazyLock::new(|| std::sync::Mutex::new(()));
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    // Shapes below mirror real `gh` output captured on 2026-07-31.

    #[test]
    fn parses_a_pull_request_with_passing_checks() {
        let j = r#"[{"number":334,"title":"fix the thing","state":"OPEN",
            "statusCheckRollup":[
                {"name":"test","status":"COMPLETED","conclusion":"SUCCESS"},
                {"name":"lint","status":"COMPLETED","conclusion":"SUCCESS"}]}]"#;
        let pr = parse_pr_json(j).unwrap();
        assert_eq!(pr.number, 334);
        assert_eq!(pr.title, "fix the thing");
        assert_eq!(pr.state, "OPEN");
        assert_eq!(pr.checks.state, CheckState::Passing);
        assert_eq!(pr.checks.passing, 2);
        assert_eq!(pr.checks.failing, 0);
    }

    #[test]
    fn a_skipped_check_is_not_a_failure() {
        // Measured: real conclusions include SKIPPED. Treating "not SUCCESS" as
        // failure paints false red on a healthy PR.
        let j = r#"[{"number":1,"title":"t","state":"OPEN",
            "statusCheckRollup":[
                {"name":"e2e","status":"COMPLETED","conclusion":"SKIPPED"},
                {"name":"unit","status":"COMPLETED","conclusion":"SUCCESS"}]}]"#;
        let pr = parse_pr_json(j).unwrap();
        assert_eq!(pr.checks.state, CheckState::Passing);
        assert_eq!(pr.checks.failing, 0);
    }

    #[test]
    fn a_completed_check_with_a_missing_conclusion_field_is_passing_not_pending() {
        // A check can finish (status COMPLETED) yet omit `conclusion` entirely --
        // gh's JSON does not guarantee the field is present. That must not be
        // read the same as a check still in flight: it already finished with
        // nothing bad reported, so it counts as passing/neutral, not pending.
        let j = r#"[{"number":1,"title":"t","state":"OPEN",
            "statusCheckRollup":[
                {"name":"neutral-check","status":"COMPLETED"}]}]"#;
        let pr = parse_pr_json(j).unwrap();
        assert_eq!(pr.checks.state, CheckState::Passing);
        assert_eq!(pr.checks.passing, 1);
        assert_eq!(pr.checks.pending, 0);
    }

    #[test]
    fn a_status_context_check_with_state_failure_is_a_failure() {
        // `statusCheckRollup` is a union of CheckRun (GitHub Actions -- has
        // status/conclusion) and StatusContext (external CI: CircleCI,
        // Jenkins, Codecov, Vercel, etc. -- has neither, only `state`). A
        // StatusContext entry captured from GitHub's live schema carries no
        // `status`/`conclusion` fields at all, only `state`. Without a
        // fallback to `state`, this reads as Pending regardless of how red
        // it actually is -- the exact inverse of "failing must win".
        let j = r#"[{"number":1,"title":"t","state":"OPEN",
            "statusCheckRollup":[
                {"context":"ci/circleci: build","state":"FAILURE"}]}]"#;
        let pr = parse_pr_json(j).unwrap();
        assert_eq!(pr.checks.state, CheckState::Failing);
        assert_eq!(pr.checks.failing, 1);
        assert_eq!(pr.checks.pending, 0);
    }

    #[test]
    fn a_failing_status_context_outranks_a_pending_check_run() {
        // Mixed rollup: a CheckRun still queued (pending) alongside a
        // StatusContext that failed. Failing must still win the rollup even
        // though the two entries come from different union variants.
        let j = r#"[{"number":1,"title":"t","state":"OPEN",
            "statusCheckRollup":[
                {"name":"build","status":"QUEUED","conclusion":""},
                {"context":"ci/circleci: test","state":"FAILURE"}]}]"#;
        let pr = parse_pr_json(j).unwrap();
        assert_eq!(pr.checks.state, CheckState::Failing);
        assert_eq!(pr.checks.failing, 1);
        assert_eq!(pr.checks.pending, 1);
    }

    #[test]
    fn a_status_context_with_state_success_is_passing() {
        let j = r#"[{"number":1,"title":"t","state":"OPEN",
            "statusCheckRollup":[
                {"context":"ci/circleci: build","state":"SUCCESS"}]}]"#;
        let pr = parse_pr_json(j).unwrap();
        assert_eq!(pr.checks.state, CheckState::Passing);
        assert_eq!(pr.checks.passing, 1);
    }

    #[test]
    fn an_in_flight_check_run_with_a_stray_state_field_is_still_pending() {
        // A genuine CheckRun never populates `state` -- this fixture is
        // synthetic, standing in for a hypothetical malformed/mixed entry.
        // The `state` fallback must be gated on BOTH conclusion and status
        // being empty, not just conclusion failing to resolve: a CheckRun
        // that is still running always has a non-empty `status` (QUEUED,
        // IN_PROGRESS, ...) even before it has a `conclusion`. If a stray
        // `state` field were consulted whenever conclusion alone was
        // unresolved, an in-flight check could be misread as Failing/Passing
        // off a field real CheckRuns never send. Status non-empty and not
        // COMPLETED must keep this Pending regardless of `state`.
        let j = r#"[{"number":1,"title":"t","state":"OPEN",
            "statusCheckRollup":[
                {"name":"build","status":"QUEUED","conclusion":"","state":"FAILURE"}]}]"#;
        let pr = parse_pr_json(j).unwrap();
        assert_eq!(pr.checks.state, CheckState::Pending);
        assert_eq!(pr.checks.pending, 1);
        assert_eq!(pr.checks.failing, 0);
    }

    #[test]
    fn a_startup_failure_conclusion_is_a_failure_not_pending() {
        // Full CheckConclusionState: ACTION_REQUIRED, TIMED_OUT, CANCELLED,
        // FAILURE, SUCCESS, NEUTRAL, SKIPPED, STARTUP_FAILURE, STALE. A
        // workflow that failed to start is unambiguously red.
        let j = r#"[{"number":1,"title":"t","state":"OPEN",
            "statusCheckRollup":[
                {"name":"build","status":"COMPLETED","conclusion":"STARTUP_FAILURE"}]}]"#;
        let pr = parse_pr_json(j).unwrap();
        assert_eq!(pr.checks.state, CheckState::Failing);
        assert_eq!(pr.checks.failing, 1);
    }

    #[test]
    fn a_queued_check_with_an_empty_conclusion_is_pending() {
        // Measured: a QUEUED check reports conclusion "".
        let j = r#"[{"number":1,"title":"t","state":"OPEN",
            "statusCheckRollup":[
                {"name":"build","status":"QUEUED","conclusion":""},
                {"name":"unit","status":"COMPLETED","conclusion":"SUCCESS"}]}]"#;
        let pr = parse_pr_json(j).unwrap();
        assert_eq!(pr.checks.state, CheckState::Pending);
        assert_eq!(pr.checks.pending, 1);
        assert_eq!(pr.checks.passing, 1);
    }

    #[test]
    fn any_failure_outranks_pending_and_passing() {
        let j = r#"[{"number":1,"title":"t","state":"OPEN",
            "statusCheckRollup":[
                {"name":"a","status":"COMPLETED","conclusion":"SUCCESS"},
                {"name":"b","status":"QUEUED","conclusion":""},
                {"name":"c","status":"COMPLETED","conclusion":"FAILURE"}]}]"#;
        let pr = parse_pr_json(j).unwrap();
        assert_eq!(pr.checks.state, CheckState::Failing);
        assert_eq!(pr.checks.failing, 1);
    }

    #[test]
    fn a_pull_request_with_no_checks_reports_none() {
        let j = r#"[{"number":9,"title":"t","state":"OPEN","statusCheckRollup":[]}]"#;
        let pr = parse_pr_json(j).unwrap();
        assert_eq!(pr.checks.state, CheckState::None);
    }

    #[test]
    fn an_empty_list_means_no_pull_request() {
        assert!(parse_pr_json("[]").is_none());
    }

    #[test]
    fn malformed_json_is_none_not_a_panic() {
        assert!(parse_pr_json("not json at all").is_none());
        assert!(parse_pr_json("").is_none());
    }

    #[test]
    fn a_missing_gh_binary_is_reported_not_panicked() {
        // Exercises fetch()'s own error-mapping arm for RunError::Spawn --
        // the case where `gh` cannot be found at all. describe_fetch_error is
        // the exact function fetch() calls via .map_err, not a parallel copy
        // of its logic, so this proves what fetch() actually does without
        // needing to make a real subprocess fail to spawn.
        //
        // A prior version of this test mutated the process-global PATH env
        // var to provoke a real spawn failure through fetch() end to end.
        // That caused a reproducible ~1-in-7 flake: git/local.rs's tests
        // spawn `git` via bare PATH lookup and take no lock against this
        // module's env mutation, since env vars are process-global and
        // cargo test runs all of a crate's tests in one process. Testing the
        // pure error-mapping function directly removes the process-global
        // mutation entirely rather than trying to serialize around it.
        let err = describe_fetch_error(RunError::Spawn("No such file or directory".into()));
        assert_eq!(err, "gh is not installed or not on PATH", "got {err:?}");
    }

    #[test]
    fn a_timeout_is_reported_as_such() {
        assert_eq!(describe_fetch_error(RunError::Timeout), "gh timed out");
    }

    #[test]
    fn exit_code_4_is_reported_as_unauthenticated_even_with_unrelated_stderr_text() {
        // Exit code 4 is the primary signal, not the stderr wording -- this
        // must classify as an auth failure even when stderr says nothing
        // about auth at all.
        let err = describe_fetch_error(RunError::Failed {
            code: Some(4),
            stderr: "some unrelated message".to_string(),
        });
        assert_eq!(err, "gh is not authenticated -- run `gh auth login`", "got {err:?}");
    }

    #[test]
    fn stderr_mentioning_auth_is_a_secondary_signal_when_the_code_is_not_4() {
        // Secondary fallback: some other exit code, but stderr still reads as
        // an auth problem.
        let err = describe_fetch_error(RunError::Failed {
            code: Some(1),
            stderr: "authentication required".to_string(),
        });
        assert_eq!(err, "gh is not authenticated -- run `gh auth login`", "got {err:?}");
    }

    #[test]
    fn an_unrelated_failure_passes_stderr_through_verbatim() {
        let err = describe_fetch_error(RunError::Failed {
            code: Some(1),
            stderr: "no pull requests found for branch \"x\"".to_string(),
        });
        assert_eq!(err, "no pull requests found for branch \"x\"");
    }

    #[test]
    #[ignore]
    fn an_unauthenticated_gh_is_reported_as_such() {
        // GH_CONFIG_DIR points gh at an empty, throwaway config directory --
        // this does NOT touch the user's real gh credentials. Combined with
        // GITHUB_TOKEN/GH_TOKEN cleared for the duration, gh has no way to
        // authenticate and exits 4, its dedicated auth-error code. Ignored
        // because it still shells out to the real gh binary (no network
        // call is made -- gh rejects locally before ever reaching GitHub --
        // but it is still a real-machine test of gh's behavior, consistent
        // with how fetches_a_real_pull_request is ignored for the same
        // reason).
        let _guard = env_lock();
        let d = tempfile::tempdir().unwrap();
        let empty_config = tempfile::tempdir().unwrap();

        let old_gh_config_dir = std::env::var("GH_CONFIG_DIR").ok();
        let old_github_token = std::env::var("GITHUB_TOKEN").ok();
        let old_gh_token = std::env::var("GH_TOKEN").ok();

        std::env::set_var("GH_CONFIG_DIR", empty_config.path());
        std::env::remove_var("GITHUB_TOKEN");
        std::env::remove_var("GH_TOKEN");

        let r = fetch(d.path(), "any-branch");

        match old_gh_config_dir {
            Some(v) => std::env::set_var("GH_CONFIG_DIR", v),
            None => std::env::remove_var("GH_CONFIG_DIR"),
        }
        match old_github_token {
            Some(v) => std::env::set_var("GITHUB_TOKEN", v),
            None => std::env::remove_var("GITHUB_TOKEN"),
        }
        match old_gh_token {
            Some(v) => std::env::set_var("GH_TOKEN", v),
            None => std::env::remove_var("GH_TOKEN"),
        }

        let err = r.expect_err("an unauthenticated gh must be reported as an error");
        println!("unauthenticated gh -> {err:?}");
        assert_eq!(err, "gh is not authenticated -- run `gh auth login`", "got {err:?}");
    }

    /// Exercises the real `gh` binary and real GitHub JSON. Point
    /// `CLAUDRON_TEST_REPO` at a git repo with a GitHub remote and an open PR:
    ///   CLAUDRON_TEST_REPO=~/code/some-repo cargo test fetches_a_real -- --ignored --nocapture
    #[test]
    #[ignore]
    fn fetches_a_real_pull_request() {
        let Ok(dir) = std::env::var("CLAUDRON_TEST_REPO") else {
            println!("set CLAUDRON_TEST_REPO to a repo with a GitHub remote; skipping");
            return;
        };
        let p = std::path::PathBuf::from(dir);
        if !p.exists() {
            println!("CLAUDRON_TEST_REPO does not exist; skipping");
            return;
        }

        // A hardcoded branch name rots the moment its PR merges -- this test has
        // already rotted twice that way (PR #334, then #333). Discover a
        // currently-open PR at runtime instead of pinning one by hand.
        let discover = run(
            "gh",
            &["pr", "list", "--state", "open", "--limit", "1", "--json", "headRefName"],
            &p,
            GH_TIMEOUT,
        );
        let Ok(out) = discover else {
            println!("gh pr list failed to discover an open PR; skipping: {discover:?}");
            return;
        };
        let branch = serde_json::from_str::<Value>(&out)
            .ok()
            .and_then(|v| v.as_array()?.first()?.get("headRefName")?.as_str().map(String::from));
        let Some(branch) = branch else {
            println!("no open pull request in CLAUDRON_TEST_REPO right now; skipping (genuine skip, not a pass)");
            return;
        };

        // Assert the call succeeds AND that it actually found the PR -- Ok alone
        // proves nothing about parsing, since a missing PR is also Ok(None). This
        // is the assertion the old hardcoded-branch version could no longer make
        // once its pinned branch's PR merged.
        let t0 = std::time::Instant::now();
        let r = fetch(&p, &branch);
        println!("elapsed {:?} -> branch={branch:?} {r:?}", t0.elapsed());
        match &r {
            Ok(g) => assert!(
                g.pull_request.is_some(),
                "discovered branch {branch:?} was reported by `gh pr list --state open` \
                 but fetch() found no pull request for it: {r:?}"
            ),
            Err(e) => panic!("gh call failed: {e:?}"),
        }

        // Run a second call against a branch that is guaranteed to have no PR,
        // and assert the two outcomes DIFFER in the way they should -- a missing
        // PR must be Ok(GitRemote { pull_request: None }), never an error.
        let none = fetch(&p, "branch-that-does-not-exist-anywhere-xyzzy");
        println!("no-such-branch -> {none:?}");
        match &none {
            Ok(g) => assert!(
                g.pull_request.is_none(),
                "a nonexistent branch must report no pull request: {none:?}"
            ),
            Err(e) => panic!("a branch with no PR must be Ok with pull_request None, not an error: {e:?}"),
        }

        assert_ne!(
            r.unwrap().pull_request.is_some(),
            none.unwrap().pull_request.is_some(),
            "the two calls must discriminate: one found a PR, the other must not"
        );
    }
}
