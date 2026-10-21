// Native glass behind the transparent webview (macOS only).
//
// CSS backdrop-filter can only blur what is inside the webview, never the desktop
// behind the window, so the real material is a native view placed underneath the
// webview and shaped as a circle that follows the UI (core while idle, full ring
// while dragging). On Windows and Linux the native effects only cover the whole
// window rectangle, so those platforms keep the CSS-only glass.

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
        // Only ever touched on the main thread, where AppKit lives.
        static GLASS: RefCell<Option<Glass>> = const { RefCell::new(None) };
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

    /// Positions a circular glass view centered at (cx, cy) in webview points.
    /// A diameter of 0 hides it. Must run on the main thread.
    pub fn set_shape(ns_window: *mut c_void, cx: f64, cy: f64, diameter: f64) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let window = unsafe { &*(ns_window as *const NSWindow) };
        let Some(content) = window.contentView() else { return };

        let bounds = content.bounds();
        let r = diameter / 2.0;
        // AppKit views use a bottom-left origin unless flipped; the webview uses top-left.
        let y = if content.isFlipped() { cy - r } else { bounds.size.height - cy - r };
        let frame = NSRect::new(NSPoint::new(cx - r, y), NSSize::new(diameter, diameter));

        GLASS.with_borrow_mut(|slot| {
            let glass = slot.get_or_insert_with(|| {
                let glass = create(mtm, frame);
                // Below the webview, so the HTML draws on top of the material
                content.addSubview_positioned_relativeTo(glass.view(), NSWindowOrderingMode::Below, None);
                glass
            });
            glass.view().setFrame(frame);
            glass.set_radius(r);
            glass.view().setHidden(diameter <= 0.0);
        });
    }
}

/// Returns whether native glass is available, so the UI can lighten its CSS tint.
#[tauri::command]
pub fn set_glass(window: tauri::WebviewWindow, cx: f64, cy: f64, diameter: f64) -> bool {
    #[cfg(target_os = "macos")]
    {
        let Ok(ns_window) = window.ns_window() else { return false };
        let ns_window = ns_window as usize;
        window
            .run_on_main_thread(move || imp::set_shape(ns_window as *mut _, cx, cy, diameter))
            .is_ok()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, cx, cy, diameter);
        false
    }
}
