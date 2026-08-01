use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The lifecycle events Claudron hooks.
///
/// Only events that CANNOT alter Claude Code's behaviour. `SessionStart` and
/// `SessionEnd` are side-effects-only regardless of exit code, while
/// `PreToolUse`, `UserPromptSubmit`, `Stop`, and `PermissionRequest` can block
/// on exit code 2. An observational feature has no business on a blocking path.
///
/// `PermissionRequest` is deliberately absent for a second reason: it fires for
/// auto-approved tools too, so mapping it to "waiting for input" produces false
/// positives on sessions that are not waiting for anything.
pub const HOOK_EVENTS: [&str; 2] = ["SessionStart", "SessionEnd"];

/// The hook script.
///
/// Parses stdin with `grep`/`sed` rather than `jq`: `jq` is not installed by
/// default on macOS, and a hook that fails on a bare machine is worse than no
/// hook. `$PPID` is the `claude` process that spawned this script -- verified
/// empirically, since no hook input or environment variable exposes the pid.
///
/// Always exits 0. A hook that fails must never surface as an error in the
/// user's session.
pub const HOOK_SCRIPT: &str = r#"#!/bin/sh
# Claudron session hook. Records which OS process is running which session so
# Claudron can identify sessions exactly rather than guessing from directories.
# Purely observational: writes one small file and always exits 0.
EVENTS_DIR="$(dirname "$0")/events"
mkdir -p "$EVENTS_DIR" 2>/dev/null || exit 0

INPUT=$(cat)
field() {
  printf '%s' "$INPUT" | grep -o "\"$1\"[[:space:]]*:[[:space:]]*\"[^\"]*\"" | sed 's/.*"\([^"]*\)"$/\1/'
}

SESSION_ID=$(field session_id)
[ -z "$SESSION_ID" ] && exit 0
EVENT=$(field hook_event_name)
CWD=$(field cwd)

# Write via a temp file and rename so a reader never sees a half-written file.
TMP="$EVENTS_DIR/.$PPID.tmp"
printf '{"event":"%s","pid":%s,"session_id":"%s","cwd":"%s","ts":%s}\n' \
  "$EVENT" "$PPID" "$SESSION_ID" "$CWD" "$(date +%s)" > "$TMP" 2>/dev/null \
  && mv "$TMP" "$EVENTS_DIR/$PPID.json" 2>/dev/null

exit 0
"#;

/// Whether Claudron's hook is present in the user's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallState {
    /// Every event Claudron needs is registered.
    Installed,
    /// Some but not all events are registered -- a partial or stale install.
    Partial,
    NotInstalled,
}

pub fn hook_command_path() -> PathBuf {
    super::hook_script_path()
}

/// The user's Claude Code settings file. Claudron does not own this.
pub fn settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("settings.json")
}

/// Whether `settings` already registers `command` for `event`.
fn event_has_command(settings: &Value, event: &str, command: &str) -> bool {
    settings
        .get("hooks")
        .and_then(|h| h.get(event))
        .and_then(Value::as_array)
        .map(|entries| {
            entries.iter().any(|entry| {
                entry
                    .get("hooks")
                    .and_then(Value::as_array)
                    .map(|hs| {
                        hs.iter()
                            .any(|h| h.get("command").and_then(Value::as_str) == Some(command))
                    })
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// Classify how much of Claudron's hook is registered.
pub fn state_of(settings: &Value, command: &str) -> InstallState {
    let present = HOOK_EVENTS
        .iter()
        .filter(|e| event_has_command(settings, e, command))
        .count();
    match present {
        0 => InstallState::NotInstalled,
        n if n == HOOK_EVENTS.len() => InstallState::Installed,
        _ => InstallState::Partial,
    }
}

/// Add Claudron's hook to `settings`, leaving everything else untouched.
///
/// Idempotent: registering twice is a no-op rather than a duplicate entry.
/// Other tools' hooks on the same event are preserved -- this appends to the
/// event's array and never replaces it.
pub fn with_hook_installed(mut settings: Value, command: &str) -> Value {
    if !settings.is_object() {
        settings = json!({});
    }
    let hooks = settings
        .as_object_mut()
        .expect("object")
        .entry("hooks")
        .or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }

    for event in HOOK_EVENTS {
        if event_has_command(&settings, event, command) {
            continue;
        }
        let hooks = settings
            .get_mut("hooks")
            .and_then(Value::as_object_mut)
            .expect("hooks object");
        let entries = hooks.entry(event).or_insert_with(|| json!([]));
        if !entries.is_array() {
            *entries = json!([]);
        }
        entries.as_array_mut().expect("array").push(json!({
            "hooks": [{
                "type": "command",
                "command": command,
                // Hooks are synchronous by default and block the triggering
                // action. An observational hook must never add latency to a
                // user's session.
                "async": true,
                "timeout": 5
            }]
        }));
    }
    settings
}

/// Remove Claudron's hook from `settings`, leaving everything else untouched.
///
/// Removes only entries naming Claudron's own command, and drops an event key
/// only once it is empty -- another tool's hooks on the same event survive.
pub fn with_hook_removed(mut settings: Value, command: &str) -> Value {
    let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) else {
        return settings;
    };

    for event in HOOK_EVENTS {
        let Some(entries) = hooks.get_mut(event).and_then(Value::as_array_mut) else {
            continue;
        };
        entries.retain(|entry| {
            let mine = entry
                .get("hooks")
                .and_then(Value::as_array)
                .map(|hs| {
                    hs.iter()
                        .any(|h| h.get("command").and_then(Value::as_str) == Some(command))
                })
                .unwrap_or(false);
            !mine
        });
        if entries.is_empty() {
            hooks.remove(event);
        }
    }
    if hooks.is_empty() {
        settings.as_object_mut().expect("object").remove("hooks");
    }
    settings
}

/// Read the user's settings, tolerating absence but never corruption.
///
/// A missing file is a legitimately empty object. A file that exists but does
/// not parse is an ERROR: treating it as empty would let the next write
/// silently discard everything the user has configured -- the same data loss
/// `annotations::load` was fixed to avoid.
pub fn read_settings(path: &Path) -> Result<Value, String> {
    match std::fs::read_to_string(path) {
        Ok(raw) if raw.trim().is_empty() => Ok(json!({})),
        Ok(raw) => serde_json::from_str(&raw).map_err(|e| {
            format!(
                "{} is not valid JSON ({e}). Refusing to overwrite it.",
                path.display()
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("could not read {}: {e}", path.display())),
    }
}

/// Write settings atomically, so an interrupted write cannot truncate the file.
fn write_settings(path: &Path, settings: &Value) -> Result<(), String> {
    let body = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("could not serialize settings: {e}"))?;
    let tmp = path.with_extension("json.claudron-tmp");
    std::fs::write(&tmp, body.as_bytes())
        .map_err(|e| format!("could not write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("could not replace {}: {e}", path.display()))
}

/// What the UI needs to describe the install before the user consents.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookPlan {
    pub state: InstallState,
    pub settings_path: String,
    pub script_path: String,
    pub events: Vec<String>,
    /// Exactly what will be added to settings.json, rendered for display.
    pub settings_snippet: String,
}

pub fn install_state() -> Result<HookPlan, String> {
    let settings = read_settings(&settings_path())?;
    let command = hook_command_path();
    let command = command.to_string_lossy().to_string();
    let snippet = serde_json::to_string_pretty(&json!({
        "hooks": HOOK_EVENTS.iter().map(|e| (e.to_string(), json!([{
            "hooks": [{"type": "command", "command": command, "async": true, "timeout": 5}]
        }]))).collect::<serde_json::Map<_, _>>()
    }))
    .unwrap_or_default();

    Ok(HookPlan {
        state: state_of(&settings, &command),
        settings_path: settings_path().to_string_lossy().to_string(),
        script_path: command,
        events: HOOK_EVENTS.iter().map(|e| e.to_string()).collect(),
        settings_snippet: snippet,
    })
}

/// Write the hook script and register it. Called only after explicit consent.
pub fn install() -> Result<HookPlan, String> {
    let dir = super::claudron_dir();
    std::fs::create_dir_all(dir.join("events"))
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;

    let script = super::hook_script_path();
    std::fs::write(&script, HOOK_SCRIPT)
        .map_err(|e| format!("could not write {}: {e}", script.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("could not make {} executable: {e}", script.display()))?;
    }

    let path = settings_path();
    let settings = read_settings(&path)?;
    let command = script.to_string_lossy().to_string();
    let updated = with_hook_installed(settings, &command);
    write_settings(&path, &updated)?;
    install_state()
}

/// Unregister the hook. Leaves the script and events on disk; removing the
/// registration is what stops Claude Code invoking it.
pub fn uninstall() -> Result<HookPlan, String> {
    let path = settings_path();
    let settings = read_settings(&path)?;
    let command = hook_command_path().to_string_lossy().to_string();
    let updated = with_hook_removed(settings, &command);
    write_settings(&path, &updated)?;
    install_state()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CMD: &str = "/Users/s/.claude/claudron/session-hook.sh";

    #[test]
    fn a_fresh_settings_file_reports_not_installed() {
        assert_eq!(state_of(&json!({}), CMD), InstallState::NotInstalled);
    }

    #[test]
    fn installing_registers_every_event() {
        let out = with_hook_installed(json!({}), CMD);
        assert_eq!(state_of(&out, CMD), InstallState::Installed);
        for e in HOOK_EVENTS {
            assert!(event_has_command(&out, e, CMD), "{e} not registered");
        }
    }

    #[test]
    fn installing_twice_does_not_duplicate() {
        let once = with_hook_installed(json!({}), CMD);
        let twice = with_hook_installed(once.clone(), CMD);
        assert_eq!(once, twice, "install must be idempotent");
    }

    #[test]
    fn installing_preserves_unrelated_settings() {
        // The user's own configuration must survive untouched -- this is their
        // file, not Claudron's.
        let before = json!({
            "permissions": {"allow": ["Bash"]},
            "statusLine": {"type": "command"},
            "alwaysThinkingEnabled": true
        });
        let after = with_hook_installed(before.clone(), CMD);
        for key in ["permissions", "statusLine", "alwaysThinkingEnabled"] {
            assert_eq!(after.get(key), before.get(key), "{key} was modified");
        }
    }

    #[test]
    fn installing_preserves_another_tools_hooks_on_the_same_event() {
        let before = json!({"hooks": {"SessionStart": [
            {"hooks": [{"type": "command", "command": "/other/tool.sh"}]}
        ]}});
        let after = with_hook_installed(before, CMD);
        let entries = after["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(entries.len(), 2, "the other tool's hook must survive");
        assert!(event_has_command(&after, "SessionStart", "/other/tool.sh"));
        assert!(event_has_command(&after, "SessionStart", CMD));
    }

    #[test]
    fn uninstalling_removes_only_claudrons_hook() {
        let with_both = with_hook_installed(
            json!({"hooks": {"SessionStart": [
                {"hooks": [{"type": "command", "command": "/other/tool.sh"}]}
            ]}}),
            CMD,
        );
        let after = with_hook_removed(with_both, CMD);
        assert_eq!(state_of(&after, CMD), InstallState::NotInstalled);
        assert!(
            event_has_command(&after, "SessionStart", "/other/tool.sh"),
            "the other tool's hook must survive uninstall"
        );
    }

    #[test]
    fn uninstalling_drops_an_event_key_only_once_it_is_empty() {
        let installed = with_hook_installed(json!({}), CMD);
        let after = with_hook_removed(installed, CMD);
        // Nothing left, so no empty scaffolding should remain in the user's file.
        assert_eq!(after.get("hooks"), None, "empty hooks key must be removed");
    }

    #[test]
    fn uninstalling_when_not_installed_changes_nothing() {
        let before = json!({"permissions": {"allow": ["Bash"]}});
        assert_eq!(with_hook_removed(before.clone(), CMD), before);
    }

    #[test]
    fn a_partial_install_is_reported_as_partial() {
        // A stale install from an older version registering fewer events.
        let partial = json!({"hooks": {"SessionStart": [
            {"hooks": [{"type": "command", "command": CMD}]}
        ]}});
        assert_eq!(state_of(&partial, CMD), InstallState::Partial);
    }

    #[test]
    fn the_hook_is_registered_async_so_it_cannot_slow_a_session() {
        let out = with_hook_installed(json!({}), CMD);
        let h = &out["hooks"]["SessionStart"][0]["hooks"][0];
        assert_eq!(
            h["async"],
            json!(true),
            "hooks block the session unless async"
        );
        assert!(h["timeout"].is_number(), "a hook must carry a timeout");
    }

    #[test]
    fn only_non_blocking_events_are_hooked() {
        // Exit code 2 on these can block a tool call or alter Claude's
        // behaviour. An observational feature must not sit on that path.
        for blocking in [
            "PreToolUse",
            "UserPromptSubmit",
            "Stop",
            "PermissionRequest",
        ] {
            assert!(
                !HOOK_EVENTS.contains(&blocking),
                "{blocking} can alter Claude Code's behaviour and must not be hooked"
            );
        }
    }

    #[test]
    fn a_missing_settings_file_reads_as_empty() {
        let got = read_settings(Path::new("/tmp/claudron-no-settings-zzz.json"));
        assert_eq!(got, Ok(json!({})));
    }

    #[test]
    fn a_corrupt_settings_file_is_an_error_not_an_empty_object() {
        // Returning empty would let the next write discard everything the user
        // has configured -- exactly the data loss annotations::load was fixed
        // to avoid.
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, "{ this is not json").unwrap();
        let got = read_settings(&p);
        assert!(got.is_err(), "corrupt settings must not read as empty");
        assert!(got.unwrap_err().contains("not valid JSON"));
    }

    #[test]
    fn the_hook_script_never_reports_failure_into_a_session() {
        assert!(
            HOOK_SCRIPT.contains("exit 0"),
            "a failing hook must not surface as an error in the user's session"
        );
        assert!(
            !HOOK_SCRIPT.contains("jq "),
            "jq is not installed by default on macOS; the hook must not depend on it"
        );
        assert!(
            HOOK_SCRIPT.contains("$PPID"),
            "the pid mapping is the whole point and comes from $PPID"
        );
        assert!(
            HOOK_SCRIPT.contains("mv "),
            "events must be written atomically or a reader can see a partial file"
        );
    }
}
