// Whether Option (macOS) / Alt (Windows) is held right now. Polled by the UI during a
// drag: OS drag events carry no modifier keys, and the window usually is not focused
// (the drag starts in Finder/Explorer), so keyboard events never reach the webview.

#[tauri::command]
pub fn option_key_down() -> bool {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSEvent, NSEventModifierFlags};
        NSEvent::modifierFlags_class().contains(NSEventModifierFlags::Option)
    }
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_MENU};
        (unsafe { GetAsyncKeyState(VK_MENU.0 as i32) } as u16 & 0x8000) != 0
    }
    // Linux: no portable way to read global modifier state (X11 vs Wayland); presets
    // are still available from the tray menu.
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        false
    }
}
