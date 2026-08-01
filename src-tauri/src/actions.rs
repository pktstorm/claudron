use std::process::Command;

/// Escape a value for safe interpolation into an AppleScript string literal.
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Quote a value for a POSIX shell command line.
///
/// `esc` handles the AppleScript string literal; this handles the SECOND
/// boundary, because iTerm2's `write text` types its argument into a shell.
/// Single quotes make the shell treat everything literally; an embedded
/// single quote is closed, escaped, and reopened ('\'').
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

/// AppleScript to open a new iTerm2 tab and resume the given session in `cwd`.
pub fn resume_script(session_id: &str, cwd: &str) -> String {
    // Two boundaries: the shell (inner, via shell_quote) and the AppleScript
    // string literal (outer, via esc).
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

/// Run an AppleScript, returning its stdout on success.
pub fn run_applescript(script: &str) -> Result<String, String> {
    let out = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("failed to run osascript: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        eprintln!("claudron: osascript failed: {err}");
        Err(err)
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

    #[test]
    fn focus_script_returns_not_found_when_no_tab_matches() {
        // A cwd no tab can be in. Should run cleanly and report not-found,
        // never a silent success.
        let script = iterm_focus_script("/tmp/claudron-no-such-dir-zzz");
        let out = run_applescript(&script);
        // Either iTerm2 answers "not-found", or it is not installed and we get Err.
        // Err means iTerm2 is unavailable in this environment, which is acceptable.
        if let Ok(s) = out {
            assert_eq!(s, "not-found");
        }
    }
}
