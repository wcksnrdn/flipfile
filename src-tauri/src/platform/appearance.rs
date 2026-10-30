// The app icon follows the user's appearance. The bundled icon (Finder, Launchpad,
// installers) is the light one; while flipfile runs, the Dock (macOS) or the window and
// tray icons (Windows/Linux) switch to the dark variant when the system is in dark mode.
// macOS "Auto" appearance, and systems whose theme can't be read, keep the light icon.

use tauri::{AppHandle, Manager, Theme};

const LIGHT: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/icons/icon.png"));
const DARK: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/icons/icon-dark.png"));

/// Whether the dark icon should be shown for this system theme
fn wants_dark(theme: Theme) -> bool {
    // macOS: "Auto" flips between light and dark during the day; it counts as light
    #[cfg(target_os = "macos")]
    if switches_automatically() {
        return false;
    }
    theme == Theme::Dark
}

/// Shows the icon for the current theme of the main window (call once at startup)
pub fn init(app: &AppHandle) {
    let theme = app.get_webview_window("main").and_then(|w| w.theme().ok()).unwrap_or(Theme::Light);
    apply(app, theme);
}

/// Shows the icon for `theme` (also called when the system theme changes)
pub fn apply(app: &AppHandle, theme: Theme) {
    let png = if wants_dark(theme) { DARK } else { LIGHT };

    // macOS: the Dock icon of the running app
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        set_dock_icon(png);
    }

    // Windows/Linux: the taskbar shows the window icon; the tray uses the app icon too
    #[cfg(not(target_os = "macos"))]
    if let Ok(image) = tauri::image::Image::from_bytes(png) {
        for window in app.webview_windows().values() {
            let _ = window.set_icon(image.clone());
        }
        if let Some(tray) = app.tray_by_id(super::tray::TRAY_ID) {
            let _ = tray.set_icon(Some(image));
        }
    }
}

#[cfg(target_os = "macos")]
fn set_dock_icon(png: &[u8]) {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    // AppKit may only be touched from the main thread (setup and window events run there)
    let Some(mtm) = MainThreadMarker::new() else { return };
    let data = NSData::with_bytes(png);
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        // SAFETY: called on the main thread with a valid image
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
    }
}

/// macOS: is the system appearance set to "Auto"?
#[cfg(target_os = "macos")]
fn switches_automatically() -> bool {
    use objc2_foundation::{NSString, NSUserDefaults};
    NSUserDefaults::standardUserDefaults().boolForKey(&NSString::from_str("AppleInterfaceStyleSwitchesAutomatically"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn both_icons_decode() {
        for png in [super::LIGHT, super::DARK] {
            let img = image::load_from_memory(png).expect("icon decodes");
            assert_eq!((img.width(), img.height()), (512, 512));
        }
    }
}
