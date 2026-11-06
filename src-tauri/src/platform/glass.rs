// Native glass behind the transparent webview (macOS only).
//
// CSS backdrop-filter can only blur what is inside the webview, never the desktop
// behind the window, so the real material is a native view placed underneath the
// webview and shaped as a circle that follows the UI (core while idle, full ring
// while dragging), or as the rounded card of the summary panel. On Windows and Linux
// the native effects only cover the whole window rectangle, so those platforms keep
// the CSS-only glass.

#[cfg(target_os = "macos")]
mod imp {
    use std::cell::RefCell;
    use std::ffi::c_void;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{msg_send, MainThreadMarker};
    use objc2_app_kit::{
        NSGlassEffectView, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
        NSVisualEffectView, NSWindow, NSWindowOrderingMode,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    enum Glass {
        // Liquid Glass, macOS 26+
        Liquid(Retained<NSGlassEffectView>),
        // Classic blur for older macOS versions
        Vibrancy(Retained<NSVisualEffectView>),
    }

    impl Glass {
        fn view(&self) -> &NSView {
            match self {
                Glass::Liquid(v) => v,
                Glass::Vibrancy(v) => v,
            }
        }

        fn set_radius(&self, radius: f64) {
            match self {
                Glass::Liquid(v) => v.setCornerRadius(radius),
                Glass::Vibrancy(v) => unsafe {
                    let layer: Option<Retained<AnyObject>> = msg_send![&**v, layer];
                    if let Some(layer) = layer {
                        let _: () = msg_send![&*layer, setCornerRadius: radius];
                        let _: () = msg_send![&*layer, setMasksToBounds: true];
                    }
                },
            }
        }
    }

    thread_local! {
        // Only ever touched on the main thread, where AppKit lives. One per window (the
        // circle's and the summary panel's), by the address of its NSWindow.
        static GLASS: RefCell<Vec<(usize, Glass)>> = const { RefCell::new(Vec::new()) };
    }

    fn create(mtm: MainThreadMarker, frame: NSRect) -> Glass {
        if AnyClass::get(c"NSGlassEffectView").is_some() {
            Glass::Liquid(NSGlassEffectView::initWithFrame(mtm.alloc(), frame))
        } else {
            let v = NSVisualEffectView::initWithFrame(mtm.alloc(), frame);
            v.setMaterial(NSVisualEffectMaterial::HUDWindow);
            v.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            // Stay blurred while the window is inactive (it usually is during a file drag)
            v.setState(NSVisualEffectState::Active);
            v.setWantsLayer(true);
            Glass::Vibrancy(v)
        }
    }

    /// Positions a glass view of `width` x `height` with rounded corners, centered at
    /// (cx, cy) in webview points. An empty size hides it. Must run on the main thread.
    pub fn set_shape(ns_window: *mut c_void, cx: f64, cy: f64, width: f64, height: f64, radius: f64) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let window = unsafe { &*(ns_window as *const NSWindow) };
        let Some(content) = window.contentView() else { return };

        let bounds = content.bounds();
        // AppKit views use a bottom-left origin unless flipped; the webview uses top-left.
        let top = cy - height / 2.0;
        let y = if content.isFlipped() { top } else { bounds.size.height - top - height };
        let frame = NSRect::new(NSPoint::new(cx - width / 2.0, y), NSSize::new(width, height));

        GLASS.with_borrow_mut(|views| {
            let index = match views.iter().position(|(owner, _)| *owner == ns_window as usize) {
                Some(index) => index,
                None => {
                    let glass = create(mtm, frame);
                    // Below the webview, so the HTML draws on top of the material
                    content.addSubview_positioned_relativeTo(glass.view(), NSWindowOrderingMode::Below, None);
                    views.push((ns_window as usize, glass));
                    views.len() - 1
                }
            };
            let glass = &views[index].1;
            glass.view().setFrame(frame);
            glass.set_radius(radius);
            glass.view().setHidden(width <= 0.0 || height <= 0.0);
        });
    }
}

/// Returns whether native glass is available, so the UI can lighten its CSS tint.
/// A circle of `diameter` (the main window), or a rounded rectangle when `width`,
/// `height` and `radius` are given (the summary panel).
#[tauri::command]
pub fn set_glass(
    window: tauri::WebviewWindow,
    cx: f64,
    cy: f64,
    diameter: f64,
    width: Option<f64>,
    height: Option<f64>,
    radius: Option<f64>,
) -> bool {
    let (width, height, radius) = (width.unwrap_or(diameter), height.unwrap_or(diameter), radius.unwrap_or(diameter / 2.0));
    // Values come from the UI: keep them sane before they reach AppKit.
    if ![cx, cy, width, height, radius].iter().all(|v| v.is_finite()) {
        return false;
    }
    let (cx, cy) = (cx.clamp(-4096.0, 4096.0), cy.clamp(-4096.0, 4096.0));
    let (width, height) = (width.clamp(0.0, 4096.0), height.clamp(0.0, 4096.0));
    let radius = radius.clamp(0.0, width.min(height) / 2.0);
    #[cfg(target_os = "macos")]
    {
        let Ok(ns_window) = window.ns_window() else { return false };
        let ns_window = ns_window as usize;
        window
            .run_on_main_thread(move || imp::set_shape(ns_window as *mut _, cx, cy, width, height, radius))
            .is_ok()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, cx, cy, width, height, radius);
        false
    }
}
