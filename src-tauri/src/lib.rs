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

/// The label of the one window `tauri.conf.json` defines. Tray actions and the
/// Dock-reopen handler both resolve the window by this label; a mismatch would
/// leave the app running with no way to show it again.
const MAIN_WINDOW: &str = "main";

/// Show, unminimize, and focus the main window.
///
/// All three steps are needed. `show` alone leaves a window that was minimized
/// before hiding still minimized, and without `set_focus` the window can appear
/// behind whatever the user is looking at -- which reads as nothing happening.
fn reveal_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window(MAIN_WINDOW) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use tauri::WindowEvent;

    tauri::Builder::default()
        .setup(|app| {
            let show = MenuItem::with_id(app, "show", "Show Claudron", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Claudron", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            TrayIconBuilder::new()
                .icon(tauri::image::Image::from_bytes(include_bytes!(
                    "../icons/trayTemplate.png"
                ))?)
                // A macOS template icon is tinted by the system: black on a light
                // menu bar, white on a dark one, and highlighted when the menu is
                // open. Without this the artwork is drawn as-is and looks wrong in
                // dark mode.
                .icon_as_template(true)
                .menu(&menu)
                // The menu must NOT open on left click, or the show-on-click
                // handler below never fires.
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => reveal_main_window(app),
                    // Close only hides, so this is the deliberate way out.
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        reveal_main_window(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing hides rather than quits: Claudron is a monitor meant to stay
            // running. Quitting would stop the poll loop and pay the ~10s cold scan
            // again on next launch, when the user meant "get this out of my way".
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
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
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            // Clicking the Dock icon fires Reopen. Without handling it, a hidden
            // window leaves the app running and unreachable -- worse than quitting,
            // because there is no obvious way back.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                reveal_main_window(app);
            }

            // Hiding the last window must not end the process; that is the whole
            // point of close-to-hide.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = &event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }

            let _ = (app, event);
        });
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

    /// The tray icon must exist, be a real PNG, and be a macOS template image.
    ///
    /// A template image is tinted by the system using ONLY its alpha channel, so
    /// every visible pixel must be black. Shipping colour art here renders as-is:
    /// invisible against a dark menu bar and wrong when highlighted. Nothing else
    /// checks this -- the asset lives on disk and the code just includes it.
    #[test]
    fn the_tray_icon_is_a_macos_template_image() {
        const TRAY: &[u8] = include_bytes!("../icons/trayTemplate.png");

        assert_eq!(&TRAY[..8], b"\x89PNG\r\n\x1a\n", "tray icon must be a PNG");

        // IHDR: width and height are big-endian u32 at bytes 16..24.
        let w = u32::from_be_bytes(TRAY[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(TRAY[20..24].try_into().unwrap());
        assert_eq!((w, h), (22, 22), "menu-bar icons are 22x22 at 1x");

        // Colour type 6 = RGBA. Alpha is what the system tints; an opaque format
        // (type 2, RGB) would have no shape to tint.
        assert_eq!(TRAY[25], 6, "tray icon must carry an alpha channel (RGBA)");

        for scale in ["@2x", "@3x"] {
            let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("icons/trayTemplate{scale}.png"));
            assert!(p.exists(), "missing retina tray asset: {}", p.display());
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
