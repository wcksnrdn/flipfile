// PDF -> images, rendered with PDFium (the PDF engine of Chrome). The shared library is
// downloaded by scripts/fetch-pdfium.mjs and bundled as a resource next to the app.

use image::DynamicImage;
use pdfium_render::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 200 DPI: sharp enough for reading and printing, without huge files.
const RENDER_DPI: f32 = 200.0;
/// Guards against giant pages (posters, CAD drawings) producing gigantic bitmaps.
const MAX_SIDE: i32 = 8000;

static RESOURCE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Called once at startup with the bundled `pdfium/` resource folder.
pub fn set_library_dir(dir: PathBuf) {
    let _ = RESOURCE_DIR.set(dir);
}

pub(crate) fn pdfium() -> Result<&'static Pdfium, String> {
    static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();
    PDFIUM
        .get_or_init(|| {
            let mut dirs: Vec<PathBuf> = RESOURCE_DIR.get().cloned().into_iter().collect();
            // `cargo test` / unusual dev setups: the folder the fetch script writes to
            if cfg!(debug_assertions) {
                dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("pdfium"));
            }
            for dir in dirs {
                if let Ok(bindings) = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(&dir)) {
                    return Ok(Pdfium::new(bindings));
                }
            }
            Pdfium::bind_to_system_library()
                .map(Pdfium::new)
                .map_err(|_| "The PDF engine is missing. Please reinstall flipfile.".to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub fn is_pdf(header: &[u8]) -> bool {
    header.starts_with(b"%PDF-")
}

pub struct PageInfo {
    pub pages: usize,
    /// Bytes of the largest page once rendered (RGBA), read without loading any page
    pub largest_bitmap: u64,
}

pub fn page_info(path: &Path) -> Result<PageInfo, String> {
    let document = open(path)?;
    let pages = document.pages();
    let scale = RENDER_DPI / 72.0;
    let side = |points: f32| u64::from(((points * scale).ceil() as u32).clamp(1, MAX_SIDE as u32));
    let largest_bitmap = pages
        .page_sizes()
        .unwrap_or_default()
        .iter()
        .map(|r| side(r.width().value) * side(r.height().value) * 4)
        .max()
        .unwrap_or(0);
    Ok(PageInfo { pages: pages.len() as usize, largest_bitmap })
}

fn open(path: &Path) -> Result<PdfDocument<'static>, String> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("PDF");
    pdfium()?.load_pdf_from_file(path, None).map_err(|e| {
        if format!("{e:?}").contains("Password") {
            format!("\"{name}\" is password-protected")
        } else {
            format!("\"{name}\" could not be opened as a PDF")
        }
    })
}

/// Renders every page in order, handing each one to `on_page` (index from 0) so that
/// large documents never have to be held in memory all at once.
pub fn render_pages(
    path: &Path,
    mut on_page: impl FnMut(usize, DynamicImage) -> Result<(), String>,
) -> Result<usize, String> {
    let document = open(path)?;
    let config = PdfRenderConfig::new()
        .scale_page_by_factor(RENDER_DPI / 72.0)
        .set_maximum_width(MAX_SIDE)
        .set_maximum_height(MAX_SIDE);

    let mut count = 0;
    for (index, page) in document.pages().iter().enumerate() {
        let bitmap = page
            .render_with_config(&config)
            .map_err(|e| format!("Page {} could not be rendered ({e})", index + 1))?;
        let image = bitmap
            .as_image()
            .map_err(|e| format!("Page {} could not be rendered ({e})", index + 1))?;
        on_page(index, image)?;
        count += 1;
    }
    Ok(count)
}
