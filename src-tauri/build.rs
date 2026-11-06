fn main() {
    // macOS: the Apple Intelligence helper behind AI summaries (intelligence/main.swift),
    // bundled next to the app binary as `externalBin` (tauri.macos.conf.json), so it has
    // to exist before tauri-build looks for it.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build_intelligence_helper();
    }

    // Declaring the app's own commands makes them subject to capabilities like plugin
    // commands: without this, every #[tauri::command] would be callable from any window.
    // Each one is then allowed explicitly in capabilities/*.json (`allow-<name>`).
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "convert_batch",
            "read_clipboard",
            "option_key_down",
            "settings_summary",
            "set_glass",
            "set_workshop_window",
            "quit_app",
            "summary_status",
            "summarize",
            "summary_run",
            "summary_redo",
            "summary_ask",
            "summary_show",
            "summary_close",
            "summary_copy",
            "summary_save",
            "summary_settings",
        ]),
    ))
    .expect("failed to run tauri-build");
}

/// Compiles flipfile-ai with the Swift compiler of the Xcode (Command Line) Tools. It
/// needs the macOS 26 SDK (FoundationModels); with an older one, a stand-in that reports
/// summaries as unavailable is bundled instead, so the app still builds and runs.
fn build_intelligence_helper() {
    use std::path::Path;
    use std::process::Command;

    let source = Path::new("intelligence/main.swift");
    println!("cargo:rerun-if-changed={}", source.display());
    let target = std::env::var("TARGET").expect("TARGET is set by cargo");
    let arch = if target.starts_with("aarch64") { "arm64" } else { "x86_64" };
    let output = Path::new("binaries").join(format!("flipfile-ai-{target}"));
    std::fs::create_dir_all("binaries").expect("failed to create src-tauri/binaries");

    // Up to date: tauri-build reruns this script for many reasons, Swift takes seconds
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if let (Some(built), Some(changed)) = (modified(&output), modified(source))
        && built >= changed
        && std::fs::read(&output).is_ok_and(|bytes| !bytes.starts_with(b"#!"))
    {
        return;
    }

    let compiled = Command::new("xcrun")
        .args(["--sdk", "macosx", "swiftc", "-O", "-parse-as-library"])
        .args(["-target", &format!("{arch}-apple-macos26.0")])
        .arg("-o")
        .arg(&output)
        .arg(source)
        .status();
    if !matches!(compiled, Ok(status) if status.success()) {
        println!("cargo:warning=flipfile-ai could not be compiled (Xcode with the macOS 26 SDK is needed); AI summaries will be unavailable in this build");
        std::fs::write(&output, "#!/bin/sh\necho '{\"event\":\"status\",\"available\":false,\"reason\":\"unsupported\"}'\n")
            .expect("failed to write the flipfile-ai stand-in");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o755));
        }
    }
}
