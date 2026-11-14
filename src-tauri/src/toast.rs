// Notification bubbles: a small window up and to the right of the circle (the other way
// round where the screen ends) that says what went wrong, or what a compression gained
// and cost. It runs the same web UI (src/main.tsx shows ToastBubble in it), sized to its
// bubble so that it never covers more than it shows, never takes the focus, and follows
// the circle when it is moved. One bubble at a time: a new one replaces the last.

use std::sync::Mutex;

use serde_json::Value;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "toast";
/// Radius of the circle in the middle of the main window
const CORE_RADIUS: f64 = 60.0;
/// The bubble's tail touches the circle here, along its diagonal
const TAIL_INSET: f64 = 0.62;
/// Space kept to the screen's edges
const MARGIN: f64 = 8.0;
/// The bubble is never larger than this
const MAX_WIDTH: f64 = 340.0;
const MAX_HEIGHT: f64 = 360.0;

/// The bubble on show (or about to be): the page picks it up when it starts
#[derive(Default)]
pub struct Toasts {
    current: Mutex<Option<Value>>,
    /// Size the page measured for the bubble, logical pixels
    size: Mutex<(f64, f64)>,
}

/// Shows a bubble (from the circle's page): `toast` is what it says, see src/toast.ts
#[tauri::command]
pub fn toast_show(app: AppHandle, toast: Value) -> Result<(), String> {
    *app.state::<Toasts>().current.lock().unwrap() = Some(toast.clone());
    if let Some(window) = app.get_webview_window(LABEL) {
        // The page measures the new bubble and calls `toast_fit`, which shows it
        let _ = window.emit_to(LABEL, "toast", toast);
        return Ok(());
    }
    WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::App("index.html".into()))
        .title("flipfile")
        .inner_size(MAX_WIDTH, 120.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .focused(false)
        .focusable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .accept_first_mouse(true)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// The bubble the page should show (it asks once when it starts)
#[tauri::command]
pub fn toast_current(app: AppHandle) -> Option<Value> {
    app.state::<Toasts>().current.lock().unwrap().clone()
}

/// The page measured its bubble: sizes the window to it, places it by the circle and
/// shows it. Returns where the circle is from the bubble ("bottom-left", ...), for the tail.
#[tauri::command]
pub fn toast_fit(app: AppHandle, width: f64, height: f64) -> String {
    let size = (width.clamp(80.0, MAX_WIDTH), height.clamp(40.0, MAX_HEIGHT));
    *app.state::<Toasts>().size.lock().unwrap() = size;
    let side = place(&app);
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.show();
    }
    side
}

/// Hides the bubble (it timed out, or was dismissed)
#[tauri::command]
pub fn toast_close(app: AppHandle) {
    *app.state::<Toasts>().current.lock().unwrap() = None;
    hide(&app);
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
}

/// The circle moved: the bubble on show moves with it
pub fn follow(app: &AppHandle) {
    let Some(window) = app.get_webview_window(LABEL) else { return };
    if window.is_visible().unwrap_or(false) {
        let side = place(app);
        let _ = window.emit_to(LABEL, "toast-side", side);
    }
}

/// Up and to the right of the circle, with the bubble's corner on the circle's diagonal;
/// flipped left and/or below where the screen ends. Returns the circle's side.
fn place(app: &AppHandle) -> String {
    let (Some(main), Some(window)) = (app.get_webview_window("main"), app.get_webview_window(LABEL)) else {
        return "bottom-left".into();
    };
    let (Ok(position), Ok(outer), Ok(scale)) = (main.outer_position(), main.outer_size(), main.scale_factor()) else {
        return "bottom-left".into();
    };
    let position = position.to_logical::<f64>(scale);
    let outer = outer.to_logical::<f64>(scale);
    let center = (position.x + outer.width / 2.0, position.y + outer.height / 2.0);
    let (width, height) = *app.state::<Toasts>().size.lock().unwrap();
    let reach = CORE_RADIUS * TAIL_INSET;

    let (mut right, mut above) = (true, true);
    let area = main.current_monitor().ok().flatten().map(|monitor| {
        let area = monitor.work_area();
        let origin = area.position.to_logical::<f64>(scale);
        let extent = area.size.to_logical::<f64>(scale);
        (origin.x, origin.y, extent.width, extent.height)
    });
    if let Some((left, top, area_width, _)) = area {
        right = center.0 + reach + width <= left + area_width - MARGIN;
        above = center.1 - reach - height >= top + MARGIN;
    }
    let mut x = if right { center.0 + reach } else { center.0 - reach - width };
    let mut y = if above { center.1 - reach - height } else { center.1 + reach };
    if let Some((left, top, area_width, area_height)) = area {
        x = x.clamp(left + MARGIN, (left + area_width - width - MARGIN).max(left + MARGIN));
        y = y.clamp(top + MARGIN, (top + area_height - height - MARGIN).max(top + MARGIN));
    }
    let _ = window.set_size(LogicalSize::new(width, height));
    let _ = window.set_position(LogicalPosition::new(x, y));
    // Where the circle is, seen from the bubble
    format!("{}-{}", if above { "bottom" } else { "top" }, if right { "left" } else { "right" })
}
