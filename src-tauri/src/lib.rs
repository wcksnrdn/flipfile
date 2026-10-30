mod convert; // conversion pipeline behind `convert_batch`
mod office; // Word <-> PDF engines
mod pdf; // low-level PDF writing and PDFium rendering
mod platform; // OS integration: clipboard, glass, keys, codecs, tray
mod state; // app state: grants, history, settings

use std::sync::Mutex;

use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use platform::{appearance, clipboard, glass, keys, tray};
use state::{grants, history, settings};

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        tray::toggle_window(app);
                    }
                })
                .build(),
        )
        .manage(grants::Grants::default())
        // Record dropped files before the UI is told about the drop (see grants.rs).
        // Tauri runs these handlers synchronously right after emitting the event, so
        // the grant is always in place before the UI can call convert_batch.
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) => {
                window.state::<grants::Grants>().replace(paths.iter().cloned());
            }
            // The system switched between light and dark: swap the app icon to match
            tauri::WindowEvent::ThemeChanged(theme) => appearance::apply(window.app_handle(), *theme),
            _ => {}
        })
        .setup(|app| {
            let history_file = app.path().app_data_dir().ok().map(|d| d.join("history.json"));
            app.manage(Mutex::new(history::History::load(history_file)));
            let settings_file = app.path().app_data_dir().ok().map(|d| d.join("settings.json"));
            app.manage(Mutex::new(settings::Settings::load(settings_file)));

            tray::setup(app.handle())?;
            appearance::init(app.handle());
            clipboard::clear_temp(app.handle());
            if let Ok(dir) = app.path().resource_dir() {
                pdf::render::set_library_dir(dir.join("pdfium"));
            }

            // Another app may already own the shortcut; the tray still works without it.
            if let Err(e) = app.global_shortcut().register(tray::SHORTCUT) {
                eprintln!("Could not register {}: {e}", tray::SHORTCUT);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            convert::convert_batch,
            clipboard::read_clipboard,
            keys::option_key_down,
            tray::settings_summary,
            quit_app,
            glass::set_glass
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, _event| {
            // macOS: clicking the Dock icon brings back a window hidden via the tray or shortcut.
            // (Windows/Linux: a hidden window leaves the taskbar; the tray icon restores it.)
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                if let Some(w) = _app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        });
}
