// Cmd/Ctrl+V support: reads files copied in Finder/Explorer, or raw image data
// (screenshots, images copied from a browser), from the system clipboard.

use std::fs;
use std::path::PathBuf;

use image::RgbaImage;
use serde::Serialize;
use tauri::{AppHandle, Manager};

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
    tauri::async_runtime::spawn_blocking(move || {
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
        let safe: String = name.chars().filter(|c| !matches!(c, '/' | '\\' | ':')).collect();
        let path = dir.join(format!("{}.png", if safe.trim().is_empty() { "Clipboard" } else { safe.trim() }));
        rgba.save(&path).map_err(|e| e.to_string())?;
        Ok(Pasted::Image { path: path.to_string_lossy().into_owned() })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Called on startup so temporary clipboard images never pile up.
pub fn clear_temp(app: &AppHandle) {
    if let Ok(dir) = temp_dir(app) {
        let _ = fs::remove_dir_all(dir);
    }
}
