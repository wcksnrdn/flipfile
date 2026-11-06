// The summary panel: a second window beside the circle that runs the same web UI
// (src/main.tsx shows SummaryPanel in it). It is created on the first summary and only
// hidden afterwards, so it opens instantly and keeps the last summary.

use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const LABEL: &str = "summary";
const WIDTH: f64 = 420.0;
const HEIGHT: f64 = 640.0;
/// Radius of the circle in the middle of the main window
const CORE_RADIUS: f64 = 60.0;
/// Space between the circle and the panel
const GAP: f64 = 22.0;
/// Space kept to the screen's edges
const MARGIN: f64 = 12.0;

/// Shows the panel beside the circle, creating it the first time. Returns whether it
/// was just created (its UI then picks up the waiting job by itself).
pub fn show(app: &AppHandle) -> tauri::Result<bool> {
    if let Some(panel) = app.get_webview_window(LABEL) {
        if !panel.is_visible().unwrap_or(false) {
            place(app, &panel);
        }
        panel.show()?;
        panel.set_focus()?;
        return Ok(false);
    }
    let panel = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Summary")
        .inner_size(WIDTH, HEIGHT)
        .min_inner_size(360.0, 420.0)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .resizable(true)
        .visible(false)
        .build()?;
    place(app, &panel);
    panel.show()?;
    panel.set_focus()?;
    Ok(true)
}

pub fn hide(app: &AppHandle) {
    if let Some(panel) = app.get_webview_window(LABEL) {
        let _ = panel.hide();
    }
}

/// Next to the circle on the side with room for it (right first), vertically centered
/// on the circle and kept inside the screen's usable area.
fn place(app: &AppHandle, panel: &WebviewWindow) {
    let Some(main) = app.get_webview_window("main") else { return };
    let (Ok(position), Ok(size), Ok(scale)) = (main.outer_position(), main.outer_size(), main.scale_factor()) else {
        return;
    };
    let position = position.to_logical::<f64>(scale);
    let size = size.to_logical::<f64>(scale);
    let center = (position.x + size.width / 2.0, position.y + size.height / 2.0);

    let (mut width, mut height) = (WIDTH, HEIGHT);
    let area = main.current_monitor().ok().flatten().map(|monitor| {
        let area = monitor.work_area();
        let origin = area.position.to_logical::<f64>(scale);
        let extent = area.size.to_logical::<f64>(scale);
        (origin.x, origin.y, extent.width, extent.height)
    });
    if let Some((_, _, _, area_height)) = area
        && height > area_height - 2.0 * MARGIN
    {
        height = (area_height - 2.0 * MARGIN).max(420.0);
        width = WIDTH.min(height * 0.7).max(360.0);
        let _ = panel.set_size(LogicalSize::new(width, height));
    }

    let mut x = center.0 + CORE_RADIUS + GAP;
    let mut y = center.1 - height / 2.0;
    if let Some((left, top, area_width, area_height)) = area {
        if x + width > left + area_width - MARGIN {
            x = center.0 - CORE_RADIUS - GAP - width;
        }
        x = x.clamp(left + MARGIN, (left + area_width - width - MARGIN).max(left + MARGIN));
        y = y.clamp(top + MARGIN, (top + area_height - height - MARGIN).max(top + MARGIN));
    }
    let _ = panel.set_position(LogicalPosition::new(x, y));
}
