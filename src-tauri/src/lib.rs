pub mod actions;
pub mod annotations;
pub mod commands;
pub mod conversation;
pub mod git;
pub mod index;
pub mod model;
pub mod process;
pub mod project;
pub mod transcript;
pub mod version;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::list_sessions,
            commands::set_annotation,
            commands::focus_session,
            commands::resume_session,
            conversation::load_conversation,
            conversation::poll_conversation,
            conversation::load_subagent,
            git::git_local,
            git::git_remote,
            git::remove_worktree,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod config_tests {
    use serde_json::Value;

    /// Every window label named by a capability must exist in tauri.conf.json.
    ///
    /// A capability lists the window labels it grants permissions to. If no
    /// window carries that label, the capability matches nothing and the window
    /// never materialises -- the app launches with zero windows and no error,
    /// which presents as a blank or missing window. This drift is invisible to
    /// the compiler, to clippy, and to every other test.
    #[test]
    fn every_capability_window_label_exists_in_the_config() {
        let conf: Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        let cap: Value = serde_json::from_str(include_str!("../capabilities/default.json"))
            .expect("capabilities/default.json");

        let labels: Vec<&str> = conf["app"]["windows"]
            .as_array()
            .expect("app.windows must be an array")
            .iter()
            .filter_map(|w| w.get("label").and_then(Value::as_str))
            .collect();

        for want in cap["windows"].as_array().expect("capability windows") {
            let want = want.as_str().expect("window label is a string");
            assert!(
                labels.contains(&want),
                "capability grants permissions to window {want:?}, but tauri.conf.json \
                 defines no window with that label (found {labels:?}). The app would \
                 launch with no window and no error."
            );
        }
    }

    /// The native titlebar must be dark, and its backdrop must match the UI.
    ///
    /// Without `theme: "Dark"` macOS draws a light titlebar above an
    /// unconditionally dark app. `backgroundColor` matters separately: it is
    /// what paints before the webview has rendered, so a mismatch shows as a
    /// white flash on launch.
    ///
    /// This is pinned to the Tailwind class `App.tsx` actually uses, because
    /// nothing else connects the two -- the window chrome lives in JSON and the
    /// UI colour lives in a class name, and neither compiler sees the other.
    ///
    /// NOTE: `include_str!` embeds these files at COMPILE time. Editing
    /// `tauri.conf.json` alone may not invalidate the build cache, so this test
    /// can assert against a stale snapshot and fail (or pass) misleadingly.
    /// `touch src/lib.rs` before re-running if a result looks impossible.
    #[test]
    fn the_titlebar_is_dark_and_matches_the_app_background() {
        let conf: Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        let w = &conf["app"]["windows"][0];

        assert_eq!(
            w["theme"].as_str(),
            Some("Dark"),
            "the app has no light mode (no `dark:` variants, no prefers-color-scheme \
             anywhere in src/), so macOS must be told to draw dark window chrome"
        );

        // neutral-900, the value of `bg-neutral-900` in App.tsx's root element.
        const NEUTRAL_900: &str = "#171717";
        assert_eq!(
            w["backgroundColor"].as_str(),
            Some(NEUTRAL_900),
            "backgroundColor must equal Tailwind neutral-900, the colour App.tsx \
             paints its root with, or launch shows a flash of the wrong colour"
        );

        let app = include_str!("../../src/App.tsx");
        assert!(
            app.contains("bg-neutral-900"),
            "App.tsx no longer uses bg-neutral-900, so {NEUTRAL_900} is now the wrong \
             backgroundColor -- update both together"
        );
    }
}
