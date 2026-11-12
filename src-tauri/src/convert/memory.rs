// A memory budget shared by every conversion running at the same time. Each file takes
// an estimate of what converting it needs before it starts, and gives it back when it
// is done: a batch of huge photos then runs only as many at once as fit in memory
// (one per core could exhaust it and make the whole computer swap), while small files
// still use every core.

use std::path::Path;
use std::sync::{Condvar, Mutex, OnceLock};

use super::input::header;
use crate::pdf::render as pdf_render;
use crate::platform::system_codec;

const MB: u64 = 1 << 20;
const GB: u64 = 1 << 30;

pub(super) struct Budget {
    total: u64,
    available: Mutex<u64>,
    freed: Condvar,
}

/// What one conversion holds; given back when dropped.
pub(super) struct Share<'a> {
    budget: &'a Budget,
    bytes: u64,
}

impl Budget {
    pub(super) fn new(total: u64) -> Self {
        Budget { total, available: Mutex::new(total), freed: Condvar::new() }
    }

    /// A quarter of the computer's memory, between 1 and 8 GB.
    pub(super) fn global() -> &'static Budget {
        static BUDGET: OnceLock<Budget> = OnceLock::new();
        BUDGET.get_or_init(|| Budget::new(physical_memory().map_or(2 * GB, |m| m / 4).clamp(GB, 8 * GB)))
    }

    /// Waits until `bytes` are free. A request for more than the whole budget waits for
    /// all of it, so a giant file still converts, on its own.
    pub(super) fn take(&self, bytes: u64) -> Share<'_> {
        let bytes = bytes.min(self.total);
        let mut available = self.available.lock().unwrap();
        while *available < bytes {
            available = self.freed.wait(available).unwrap();
        }
        *available -= bytes;
        Share { budget: self, bytes }
    }
}

impl Drop for Share<'_> {
    fn drop(&mut self) {
        *self.budget.available.lock().unwrap() += self.bytes;
        self.budget.freed.notify_all();
    }
}

/// What converting `path` to `format` needs at most, from its header alone: the decoded
/// image plus the encoder's own working memory (AVIF's encoder needs the most).
pub(super) fn cost(path: &str, format: &str) -> u64 {
    let path = Path::new(path);
    // The speech model, its working memory and the recording's samples (an hour of
    // 16 kHz mono is 230 MB); one transcription runs at a time anyway
    if format.starts_with("tr-") {
        return 1536 * MB;
    }
    let head = header(path);
    if pdf_render::is_pdf(&head) {
        // Pages are rendered a few at a time, within `pdf::PAGE_MEMORY`
        return 384 * MB;
    }
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let pixels = image::ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .map(|(w, h)| match format {
            // The canvas, as wide as the longest side
            "square" => u64::from(w.max(h).min(super::tools::SQUARE_MAX)).pow(2),
            // The photo and its enlargement
            "up-2x" => u64::from(w) * u64::from(h) * 5,
            "up-4x" => u64::from(w) * u64::from(h) * 17,
            _ => u64::from(w) * u64::from(h),
        })
        // HEIC holds about 1/24 of its pixels' size
        .or_else(|| system_codec::is_heif_family(&head).then_some(size * 24 / 4));
    if let Some(pixels) = pixels {
        return pixels * 4 * match format {
            "avif" => 8,
            "webp" | "heic" | "gif" | "gif-anim" => 3,
            _ => 2,
        };
    }
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    match ext.as_str() {
        // Every cell becomes a positioned box when typeset: a 160 KB workbook of 3000
        // rows takes ~500 MB
        "xlsx" | "xlsm" | "xltx" | "xltm" | "xls" | "ods" | "csv" | "tsv" => (size * 4096).clamp(256 * MB, 2 * GB),
        // Other documents (typeset in memory), audio and video (other processes)
        _ => (size * 64).clamp(128 * MB, GB),
    }
}

#[cfg(unix)]
fn physical_memory() -> Option<u64> {
    let pages = unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) };
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    (pages > 0 && page_size > 0).then(|| pages as u64 * page_size as u64)
}

#[cfg(windows)]
fn physical_memory() -> Option<u64> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    unsafe { GlobalMemoryStatusEx(&mut status) }.ok()?;
    Some(status.ullTotalPhys)
}

#[cfg(not(any(unix, windows)))]
fn physical_memory() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn never_lends_more_than_it_has() {
        let budget = Budget::new(100);
        let held = AtomicU64::new(0);
        let peak = AtomicU64::new(0);
        std::thread::scope(|s| {
            for i in 0..16u64 {
                let (budget, held, peak) = (&budget, &held, &peak);
                s.spawn(move || {
                    let bytes = 10 + (i * 7) % 50;
                    let _share = budget.take(bytes);
                    let now = held.fetch_add(bytes, Ordering::SeqCst) + bytes;
                    peak.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(std::time::Duration::from_millis(2));
                    held.fetch_sub(bytes, Ordering::SeqCst);
                });
            }
        });
        assert!(peak.load(Ordering::SeqCst) <= 100);
        assert_eq!(*budget.available.lock().unwrap(), 100);
        // More than the whole budget still runs, alone
        drop(budget.take(1000));
        assert_eq!(*budget.available.lock().unwrap(), 100);
    }

    #[test]
    fn knows_the_computers_memory() {
        assert!(physical_memory().is_some_and(|m| m >= 512 * MB));
        let total = Budget::global().total;
        assert!((GB..=8 * GB).contains(&total));
    }
}
