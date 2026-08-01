//! Authoritative session identity from Claude Code hook events.
//!
//! Without hooks, liveness is inferred: `lsof` reports each `claude` process's
//! working directory and a session is called live when a process shares its
//! directory. Two sessions in one repository are indistinguishable that way,
//! and the inference is what limits per-session anything.
//!
//! A hook turns that into fact. Claude Code runs a command on lifecycle events
//! and passes JSON on stdin carrying `session_id`, `transcript_path`, and
//! `cwd`. The hook writes that to a file keyed by its own `$PPID`.
//!
//! **`$PPID` is the `claude` process.** Hook commands are spawned as children
//! of the session's process, so the hook's parent is the thing we want to
//! identify. This is an implementation detail of how Claude Code spawns hooks,
//! not a documented API guarantee, so nothing here trusts it blindly: a
//! recorded pid is only believed when a live `claude` process still carries it.
//!
//! Everything degrades. With no hooks installed there are no event files, the
//! reader returns an empty map, and callers fall back to the cwd inference that
//! shipped before this module existed.

pub mod install;
pub mod read;

pub use install::{hook_command_path, install_state, InstallState};
pub use read::{read_events, HookEvent};

use std::path::PathBuf;

/// Where Claudron keeps its own hook script and the events it writes.
///
/// Deliberately under `~/.claude/claudron/` rather than beside the user's own
/// settings: everything Claudron owns is in one directory that can be removed
/// wholesale without touching anything Claude Code depends on.
pub fn claudron_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("claudron")
}

pub fn events_dir() -> PathBuf {
    claudron_dir().join("events")
}

pub fn hook_script_path() -> PathBuf {
    claudron_dir().join("session-hook.sh")
}

/// Describe the current install without changing anything.
#[tauri::command]
pub fn hook_status() -> Result<install::HookPlan, String> {
    install::install_state()
}

/// Install the hook. Called only after the user consents in the UI.
#[tauri::command]
pub fn install_hooks() -> Result<install::HookPlan, String> {
    install::install()
}

/// Remove Claudron's hook registration.
#[tauri::command]
pub fn uninstall_hooks() -> Result<install::HookPlan, String> {
    install::uninstall()
}

#[cfg(test)]
mod e2e {
    /// Install for real, confirm the shape, then remove and confirm the user's
    /// settings are byte-identical to before.
    ///
    /// `#[ignore]`d: it writes to the real `~/.claude/settings.json`.
    /// Run with `cargo test hooks::e2e -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn installs_and_uninstalls_without_disturbing_the_users_settings() {
        let path = super::install::settings_path();
        let before = std::fs::read_to_string(&path).expect("settings must exist for this test");

        let plan = super::install::install().expect("install");
        println!("after install: {:?}", plan.state);
        assert_eq!(plan.state, super::install::InstallState::Installed);
        assert!(super::hook_script_path().exists(), "script must be written");

        let plan = super::install::uninstall().expect("uninstall");
        println!("after uninstall: {:?}", plan.state);
        assert_eq!(plan.state, super::install::InstallState::NotInstalled);

        let after = std::fs::read_to_string(&path).expect("settings still readable");
        let a: serde_json::Value = serde_json::from_str(&before).unwrap();
        let b: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert_eq!(a, b, "uninstall must leave the user's settings unchanged");
        println!("settings restored identically");
    }
}
