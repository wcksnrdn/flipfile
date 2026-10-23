fn main() {
    // Declaring the app's own commands makes them subject to capabilities like plugin
    // commands: without this, every #[tauri::command] would be callable from any window.
    // Each one is then allowed explicitly in capabilities/default.json (`allow-<name>`).
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "convert_batch",
            "read_clipboard",
            "option_key_down",
            "settings_summary",
            "set_glass",
            "quit_app",
        ]),
    ))
    .expect("failed to run tauri-build");
}
