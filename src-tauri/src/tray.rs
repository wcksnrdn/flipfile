// Menu bar / system tray icon: show/hide, recent conversions, undo, quit.

use std::path::Path;
use std::sync::Mutex;

use serde::Serialize;
#[cfg(target_os = "macos")]
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
#[cfg(not(target_os = "macos"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_opener::OpenerExt;

use crate::history::History;
use crate::settings::{Settings, QUALITY_OPTIONS, RESIZE_OPTIONS};

pub const SHORTCUT: &str = "CmdOrCtrl+Shift+K";
const TRAY_ID: &str = "main";
const RECENT_LIMIT: usize = 10;
const REVEAL_PREFIX: &str = "reveal:";
const RESIZE_PREFIX: &str = "resize:";
const QUALITY_PREFIX: &str = "quality:";

#[derive(Clone, Serialize)]
pub struct Undone {
    count: usize,
    error: Option<String>,
}

/// Brings the window to the front, or hides it if it already is in front.
pub fn toggle_window(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else { return };
    if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) {
        let _ = w.hide();
    } else {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn file_label(path: &str) -> String {
    let name = Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path);
    // Keep long names from stretching the menu
    if name.chars().count() > 40 {
        let head: String = name.chars().take(24).collect();
        let tail: String = name.chars().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect();
        format!("{head}…{tail}")
    } else {
        name.to_string()
    }
}

fn as_refs(items: &[CheckMenuItem<Wry>]) -> Vec<&dyn IsMenuItem<Wry>> {
    items.iter().map(|i| i as &dyn IsMenuItem<Wry>).collect()
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let history = app.state::<Mutex<History>>();
    let history = history.lock().unwrap();
    let recent = history.recent(RECENT_LIMIT);
    let undo_count = history.last_batch().len();
    drop(history);

    let toggle = MenuItem::with_id(app, "toggle", "Show / Hide flipfile", true, Some(SHORTCUT))?;

    let mut recent_items: Vec<Box<dyn IsMenuItem<Wry>>> = Vec::new();
    if recent.is_empty() {
        recent_items.push(Box::new(MenuItem::with_id(app, "none", "No conversions yet", false, None::<&str>)?));
    } else {
        for path in &recent {
            let id = format!("{REVEAL_PREFIX}{path}");
            recent_items.push(Box::new(MenuItem::with_id(app, id, file_label(path), true, None::<&str>)?));
        }
        recent_items.push(Box::new(PredefinedMenuItem::separator(app)?));
        recent_items.push(Box::new(MenuItem::with_id(app, "clear", "Clear History", true, None::<&str>)?));
    }
    let refs: Vec<&dyn IsMenuItem<Wry>> = recent_items.iter().map(|i| i.as_ref()).collect();
    let recent_menu = Submenu::with_items(app, "Recent Conversions", true, &refs)?;

    let undo_label = match undo_count {
        0 => "Undo Last Conversion".to_string(),
        1 => "Undo Last Conversion (1 file)".to_string(),
        n => format!("Undo Last Conversion ({n} files)"),
    };
    let undo = MenuItem::with_id(app, "undo", undo_label, undo_count > 0, None::<&str>)?;

    // Defaults for every conversion; holding Option/Alt while dragging overrides them once.
    let settings = app.state::<Mutex<Settings>>();
    let settings = settings.lock().unwrap();
    let resize_items = RESIZE_OPTIONS
        .iter()
        .map(|o| {
            let id = format!("{RESIZE_PREFIX}{}", o.id);
            CheckMenuItem::with_id(app, id, o.menu, true, settings.resize_id() == o.id, None::<&str>)
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let quality_items = QUALITY_OPTIONS
        .iter()
        .map(|o| {
            let id = format!("{QUALITY_PREFIX}{}", o.id);
            CheckMenuItem::with_id(app, id, o.menu, true, settings.quality_id() == o.id, None::<&str>)
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    drop(settings);
    let resize_menu = Submenu::with_items(app, "Resize", true, &as_refs(&resize_items))?;
    let mut quality_refs = as_refs(&quality_items);
    let quality_note = MenuItem::with_id(app, "quality-note", "Applies to JPG, AVIF and PDF", false, None::<&str>)?;
    let quality_sep = PredefinedMenuItem::separator(app)?;
    quality_refs.push(&quality_sep);
    quality_refs.push(&quality_note);
    let quality_menu = Submenu::with_items(app, "Quality", true, &quality_refs)?;
    let quit = MenuItem::with_id(app, "quit", "Quit flipfile", true, Some("CmdOrCtrl+Q"))?;

    Menu::with_items(
        app,
        &[
            &toggle,
            &PredefinedMenuItem::separator(app)?,
            &recent_menu,
            &undo,
            &PredefinedMenuItem::separator(app)?,
            &resize_menu,
            &quality_menu,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )
}

/// Rebuilds the menu after the history changed.
pub fn refresh(app: &AppHandle) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), build_menu(app)) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn undo_last(app: &AppHandle) {
    let files = app.state::<Mutex<History>>().lock().unwrap().last_batch();
    // Moved to the Trash / Recycle Bin rather than deleted, so it can still be restored
    let result = trash::delete_all(&files);
    if result.is_ok() {
        app.state::<Mutex<History>>().lock().unwrap().forget_last_batch();
    }
    let _ = app.emit(
        "history-undone",
        Undone {
            count: if result.is_ok() { files.len() } else { 0 },
            error: result.err().map(|e| e.to_string()),
        },
    );
    refresh(app);
}

/// Short description of non-default resize/quality settings, shown under the core.
#[tauri::command]
pub fn settings_summary(app: AppHandle) -> Option<String> {
    app.state::<Mutex<Settings>>().lock().unwrap().summary()
}

fn change_settings(app: &AppHandle, change: impl FnOnce(&mut Settings)) {
    let summary = {
        let state = app.state::<Mutex<Settings>>();
        let mut settings = state.lock().unwrap();
        change(&mut settings);
        settings.summary()
    };
    let _ = app.emit("settings-changed", summary);
    refresh(app); // moves the check marks
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();
    match id {
        "toggle" => toggle_window(app),
        "undo" => undo_last(app),
        "clear" => {
            app.state::<Mutex<History>>().lock().unwrap().clear();
            refresh(app);
        }
        "quit" => app.exit(0),
        _ => {
            if let Some(path) = id.strip_prefix(REVEAL_PREFIX) {
                let _ = app.opener().reveal_item_in_dir(path);
            } else if let Some(choice) = id.strip_prefix(RESIZE_PREFIX) {
                change_settings(app, |s| s.set_resize(choice));
            } else if let Some(choice) = id.strip_prefix(QUALITY_PREFIX) {
                change_settings(app, |s| s.set_quality(choice));
            }
        }
    }
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("flipfile")
        .menu(&build_menu(app)?)
        .on_menu_event(on_menu_event);

    // macOS: a monochrome template icon that adapts to light/dark menu bars, menu on click.
    #[cfg(target_os = "macos")]
    let builder = builder
        .icon(Image::from_bytes(include_bytes!("../icons/tray-template.png"))?)
        .icon_as_template(true)
        .show_menu_on_left_click(true);

    // Windows/Linux: taskbars can be light or dark, so use the colored app icon.
    // Left click toggles the window, right click opens the menu (Windows). Linux tray
    // implementations do not report clicks, so there the menu is the only entry point.
    #[cfg(not(target_os = "macos"))]
    let builder = {
        let builder = match app.default_window_icon() {
            Some(icon) => builder.icon(icon.clone()),
            None => builder,
        };
        builder.show_menu_on_left_click(false).on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                toggle_window(tray.app_handle());
            }
        })
    };

    builder.build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tauri_plugin_global_shortcut::Shortcut;

    #[test]
    fn shortcut_parses() {
        super::SHORTCUT.parse::<Shortcut>().expect("valid shortcut");
    }

    #[test]
    fn long_names_are_shortened() {
        assert_eq!(super::file_label("/a/photo.png"), "photo.png");
        let long = format!("/x/{}_converted.png", "a".repeat(60));
        assert!(super::file_label(&long).chars().count() <= 37);
        assert!(super::file_label(&long).ends_with("onverted.png"));
    }
}
