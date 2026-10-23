// Cmd/Ctrl+V support: reads files copied in Finder/Explorer, or raw image data
// (screenshots, images copied from a browser), from the system clipboard.

use std::fs;
use std::path::PathBuf;

use image::RgbaImage;
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::state::grants::Grants;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Pasted {
    Files { paths: Vec<String> },
    // Saved to a temporary PNG so it can go through the normal conversion pipeline
    Image { path: String },
    Empty,
}

fn temp_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_cache_dir()
        .map(|d| d.join("clipboard"))
        .map_err(|e| e.to_string())
}

/// `name` is the file name (without extension) for a pasted image. It comes from the
/// UI, which already knows the local date/time and formats it for the user.
#[tauri::command]
pub async fn read_clipboard(app: AppHandle, name: String) -> Result<Pasted, String> {
    let dir = temp_dir(&app)?;
    let pasted = tauri::async_runtime::spawn_blocking(move || {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;

        // Copied files take priority: Finder also puts the file's icon on the clipboard as an image.
        if let Ok(files) = clipboard.get().file_list() {
            let paths: Vec<String> = files
                .into_iter()
                .filter(|p| p.is_file())
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            if !paths.is_empty() {
                return Ok(Pasted::Files { paths });
            }
        }

        let image = match clipboard.get_image() {
            Ok(image) => image,
            Err(arboard::Error::ContentNotAvailable) => return Ok(Pasted::Empty),
            Err(e) => return Err(e.to_string()),
        };
        let rgba = RgbaImage::from_raw(image.width as u32, image.height as u32, image.bytes.into_owned())
            .ok_or("The clipboard image is damaged")?;

        // Only the latest paste is ever needed, so older temporary images are removed.
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(format!("{}.png", safe_file_name(&name)));
        rgba.save(&path).map_err(|e| e.to_string())?;
        Ok(Pasted::Image { path: path.to_string_lossy().into_owned() })
    })
    .await
    .map_err(|e| e.to_string())??;

    // What was pasted becomes the only thing convert_batch will accept (see grants.rs)
    let granted: Vec<PathBuf> = match &pasted {
        Pasted::Files { paths } => paths.iter().map(PathBuf::from).collect(),
        Pasted::Image { path } => vec![PathBuf::from(path)],
        Pasted::Empty => vec![],
    };
    app.state::<Grants>().replace(granted);
    Ok(pasted)
}

/// The UI suggests the name, but it must never escape the temp folder or produce odd
/// files: only letters, digits, spaces and `-_.` survive, no leading dot, at most 100 chars.
fn safe_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
        .take(100)
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim();
    if cleaned.is_empty() { "Clipboard".into() } else { cleaned.into() }
}

/// Called on startup so temporary clipboard images never pile up.
pub fn clear_temp(app: &AppHandle) {
    if let Ok(dir) = temp_dir(app) {
        let _ = fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::safe_file_name;

    #[test]
    fn pasted_image_names_are_sanitised() {
        assert_eq!(safe_file_name("Clipboard 2026-10-20 at 14.03.12"), "Clipboard 2026-10-20 at 14.03.12");
        assert_eq!(safe_file_name("../../etc/passwd"), "etcpasswd");
        assert_eq!(safe_file_name("..\\..\\x:y"), "xy");
        assert_eq!(safe_file_name("...hidden"), "hidden");
        assert_eq!(safe_file_name("a\u{0}b\nc"), "abc");
        assert_eq!(safe_file_name("   "), "Clipboard");
        assert_eq!(safe_file_name(&"x".repeat(500)).len(), 100);
    }
}
