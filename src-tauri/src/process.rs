use std::io::Read as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct LiveProcess {
    pub pid: i32,
    pub cwd: Option<String>,
}

/// Extract PIDs of bare `claude` CLI processes from `ps -eo pid=,comm=` output.
///
/// Must match the CLI only -- the Claude desktop app and its Electron helpers
/// also match a naive "claude" substring search.
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
        if basename != "claude" {
            continue;
        }
        // Defense in depth: don't rely solely on the basename's letter case to
        // exclude the desktop app. Every macOS application bundle executable
        // lives under a path containing ".app/Contents"; no bare CLI
        // invocation does. If the basename match ever stops holding (e.g. a
        // future desktop app build ships a lowercase `claude` binary inside
        // its bundle), this still keeps it out rather than silently admitting
        // a phantom session.
        if comm.contains(".app/Contents") {
            continue;
        }
        pids.push(pid);
    }
    pids
}

/// Run lsof for one pid, giving up after `timeout`. A hung lsof (stale network
/// mount) must never block the poll cycle that calls this.
fn lsof_cwd_with_timeout(pid: i32, timeout: Duration) -> Option<String> {
    let mut child = Command::new("lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + timeout;
    let exited = loop {
        match child.try_wait() {
            Ok(Some(_status)) => break true,
            Ok(None) => {
                if Instant::now() >= deadline {
                    break false;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => break false,
        }
    };

    if !exited {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }

    let mut stdout = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut stdout);
    }

    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix('n') {
            return Some(rest.to_string());
        }
    }
    None
}

pub fn cwd_for_pid(pid: i32) -> Option<String> {
    lsof_cwd_with_timeout(pid, Duration::from_secs(2))
}

pub fn discover_claude_processes() -> Vec<LiveProcess> {
    let Ok(out) = Command::new("ps").args(["-eo", "pid=,comm="]).output() else {
        return Vec::new();
    };
    let pids = parse_ps_output(&String::from_utf8_lossy(&out.stdout));

    let handles: Vec<_> = pids
        .into_iter()
        .map(|pid| std::thread::spawn(move || (pid, cwd_for_pid(pid))))
        .collect();

    let mut results: Vec<(i32, Option<String>)> = handles
        .into_iter()
        .filter_map(|h| h.join().ok())
        .collect();
    results.sort_by_key(|(pid, _)| *pid);

    results
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
