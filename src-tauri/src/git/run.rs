use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Why a subprocess did not produce usable output.
#[derive(Debug, Clone, PartialEq)]
pub enum RunError {
    /// The program could not be started at all -- not on PATH, not executable.
    Spawn(String),
    /// It ran past its deadline and was killed.
    Timeout,
    /// It exited non-zero. `stderr` is the tool's own message, kept verbatim so
    /// the UI can show git's wording rather than a paraphrase of it.
    Failed { code: Option<i32>, stderr: String },
}

/// Run a program to completion, or kill it at `timeout`.
///
/// Every `git` and `gh` invocation in this feature goes through here. Phase 1's
/// per-PID `lsof` calls shipped without a timeout, and one hung call would have
/// blocked the poll loop indefinitely; that is not repeated.
pub fn run(
    program: &str,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
) -> Result<String, RunError> {
    let mut child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| RunError::Spawn(e.to_string()))?;

    // Drain both pipes on their own threads, concurrently with the wait below.
    // Reading only after exit deadlocks: a child writing past the OS pipe
    // buffer (~64KB) blocks on write, never exits, and the timeout fires on a
    // command that actually succeeded. This is what Command::output() does
    // internally, and why it exists -- we need its draining plus a timeout it
    // does not offer.
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out_handle = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(p) = out_pipe.as_mut() {
            let _ = p.read_to_string(&mut s);
        }
        s
    });
    let err_handle = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(p) = err_pipe.as_mut() {
            let _ = p.read_to_string(&mut s);
        }
        s
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    // Killing the child closes the pipes, so the reader threads
                    // finish; join them rather than detaching.
                    let _ = out_handle.join();
                    let _ = err_handle.join();
                    return Err(RunError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(RunError::Spawn(e.to_string())),
        }
    };

    let stdout = out_handle.join().unwrap_or_default();
    let stderr = err_handle.join().unwrap_or_default();

    if status.success() {
        Ok(stdout)
    } else {
        Err(RunError::Failed { code: status.code(), stderr: stderr.trim().to_string() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_stdout_on_success() {
        let d = tempfile::tempdir().unwrap();
        let out = run("echo", &["hello"], d.path(), Duration::from_secs(5)).unwrap();
        assert_eq!(out.trim(), "hello");
    }

    #[test]
    fn a_missing_program_is_a_spawn_error() {
        let d = tempfile::tempdir().unwrap();
        let e = run("claudron-no-such-program", &[], d.path(), Duration::from_secs(5))
            .unwrap_err();
        assert!(matches!(e, RunError::Spawn(_)), "got {e:?}");
    }

    #[test]
    fn a_nonzero_exit_carries_stderr_verbatim() {
        let d = tempfile::tempdir().unwrap();
        let e = run("sh", &["-c", "echo bad things >&2; exit 3"], d.path(), Duration::from_secs(5))
            .unwrap_err();
        match e {
            RunError::Failed { code, stderr } => {
                assert_eq!(code, Some(3));
                assert!(stderr.contains("bad things"), "got {stderr:?}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn a_slow_program_times_out_rather_than_hanging() {
        let d = tempfile::tempdir().unwrap();
        let t0 = Instant::now();
        let e = run("sleep", &["30"], d.path(), Duration::from_millis(300)).unwrap_err();
        assert_eq!(e, RunError::Timeout);
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "took {:?} -- the timeout did not fire",
            t0.elapsed()
        );
    }

    #[test]
    fn runs_in_the_given_directory() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("marker.txt"), b"x").unwrap();
        let out = run("ls", &[], d.path(), Duration::from_secs(5)).unwrap();
        assert!(out.contains("marker.txt"), "got {out:?}");
    }

    #[test]
    fn large_output_does_not_deadlock_into_a_false_timeout() {
        // ~1MB of stdout, far past the ~64KB pipe buffer. Reading only after
        // exit makes the child block on write and the timeout fire on a
        // command that in fact succeeded.
        let d = tempfile::tempdir().unwrap();
        let out = run(
            "sh",
            &["-c", "yes 0123456789abcdef | head -65536"],
            d.path(),
            Duration::from_secs(10),
        )
        .expect("a large but successful command must not time out");
        assert!(out.len() > 1_000_000, "expected ~1MB, got {} bytes", out.len());
    }

    #[test]
    fn large_stderr_also_does_not_deadlock() {
        // The same trap exists on the stderr pipe.
        let d = tempfile::tempdir().unwrap();
        let e = run(
            "sh",
            &["-c", "yes 0123456789abcdef | head -65536 >&2; exit 1"],
            d.path(),
            Duration::from_secs(10),
        )
        .unwrap_err();
        match e {
            RunError::Failed { stderr, .. } => {
                assert!(stderr.len() > 1_000_000, "expected ~1MB, got {}", stderr.len())
            }
            other => panic!("expected Failed with large stderr, got {other:?}"),
        }
    }
}
