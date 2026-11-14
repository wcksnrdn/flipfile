// Live estimates for the Option/Alt ring: what each level would make of the dropped files,
// worked out while the user points, so the slices can say "≈ 610 KB" and the centre what
// it costs. Photos and PDFs are really compressed, in memory; videos and recordings are
// worked out from their length and the level's bitrate. With many files, a few of them
// stand for all. Results are kept per file and level while the file doesn't change.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use super::{av, consequence, kind_of, pdf, photo, Kind, Level, LEVELS};
use crate::convert::input::{file_name, open_oriented, same_format_ext};
use crate::state::grants::Grants;

/// Files measured to stand for all of them
const SAMPLES: usize = 3;
/// PDFs larger than this aren't measured (each level holds the whole document)
const MAX_PDF_ESTIMATE: u64 = 100 * 1024 * 1024;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    /// Echoed from the request, so the UI can drop answers to an older drag
    token: u32,
    /// The ring item: "cmp-light" ...
    id: &'static str,
    /// Bytes of all the files now
    before: u64,
    /// Bytes after this level (never more than `before`), when it could be worked out
    after: Option<u64>,
    /// What it costs: "Fine detail softens · max 1920 px", "PNG → JPG · ..."
    note: String,
    /// Worked out from a sample or a bitrate rather than measured on every file
    approximate: bool,
}

/// One file's result at one level: its bytes, and the format it changes to
type Measured = (u64, Option<String>);

static CACHE: Mutex<Option<HashMap<(PathBuf, u64, SystemTime, Level), Option<Measured>>>> = Mutex::new(None);

#[tauri::command]
pub async fn compress_estimate(app: AppHandle, paths: Vec<String>, token: u32, on_estimate: Channel<Estimate>) -> Result<(), String> {
    if let Err(denied) = app.state::<Grants>().check_measure(&paths) {
        return Err(format!("\"{}\" was not given to flipfile", file_name(Path::new(denied))));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        estimate(&paths, token, &|estimate| {
            let _ = on_estimate.send(estimate);
        });
    })
    .await
    .map_err(|e| e.to_string())
}

/// Every level's estimate for `paths`, each handed to `send` as soon as it is worked out
/// (the levels are worked out side by side)
fn estimate(paths: &[PathBuf], token: u32, send: &(dyn Fn(Estimate) + Sync)) {
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let before: u64 = paths.iter().map(|p| size(p)).sum();
    let kinds: Vec<Option<Kind>> = paths.iter().map(|p| kind_of(p)).collect();
    let kind = kinds.first().copied().flatten().filter(|k| kinds.iter().all(|other| *other == Some(*k)));
    // The largest files of the batch stand for it
    let mut samples: Vec<&PathBuf> = paths.iter().zip(&kinds).filter(|(_, k)| k.is_some()).map(|(p, _)| p).collect();
    samples.sort_by_key(|p| std::cmp::Reverse(size(p)));
    samples.truncate(SAMPLES);

    let one = |level: Level| {
            let measured: Vec<(u64, Option<Measured>)> = samples.iter().map(|p| (size(p), cached(p, level))).collect();
            let known: Vec<(u64, Measured)> = measured.iter().filter_map(|(b, m)| m.clone().map(|m| (*b, m))).collect();
            let sample_before: u64 = known.iter().map(|(b, _)| b).sum();
            let sample_after: u64 = known.iter().map(|(b, (a, _))| (*a).min(*b)).sum();
            let after = (sample_before > 0).then(|| ((before as f64) * (sample_after as f64 / sample_before as f64)).round() as u64);
            let measured_all = known.len() == paths.len();
            let by_bitrate = matches!(kind, Some(Kind::Video | Kind::Audio));
            let change = known.iter().find_map(|(_, (_, c))| c.clone());
            let note = match (kind, &change) {
                (Some(kind), Some(change)) => format!("{change} · {}", consequence(kind, level)),
                (Some(kind), None) => consequence(kind, level).to_string(),
                (None, _) => "Each file in its own way".to_string(),
            };
            Estimate { token, id: level.id(), before, after, note, approximate: !measured_all || by_bitrate }
    };
    std::thread::scope(|scope| {
        for level in LEVELS {
            scope.spawn(move || send(one(level)));
        }
    });
}

/// One file at one level, measured once per version of the file
fn cached(path: &Path, level: Level) -> Option<Measured> {
    let meta = std::fs::metadata(path).ok()?;
    let key = (path.to_path_buf(), meta.len(), meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), level);
    if let Some(hit) = CACHE.lock().unwrap().as_ref().and_then(|c| c.get(&key)) {
        return hit.clone();
    }
    let result = std::panic::catch_unwind(|| measure(path, level, meta.len())).ok().flatten();
    CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, result.clone());
    result
}

fn measure(path: &Path, level: Level, bytes: u64) -> Option<Measured> {
    let changed = |ext: &str| super::changed(path, ext);
    match kind_of(path)? {
        Kind::Photo => {
            let own = same_format_ext(path).ok()?;
            let (img, _) = open_oriented(path).ok()?;
            let (encoded, ext) = photo::encode_level(&img, &own, level).ok()?;
            Some((encoded.len() as u64, changed(&ext)))
        }
        Kind::Pdf => {
            // Four levels side by side each hold the document: very large PDFs only say
            // what the level does
            if bytes > MAX_PDF_ESTIMATE {
                return None;
            }
            let data = std::fs::read(path).ok()?;
            Some((pdf::compress_bytes(&data, level).ok()?.len() as u64, None))
        }
        Kind::Video => {
            let seconds = av::duration(path)?;
            let plan = av::plan_video(level, bytes, Some(seconds));
            let after = seconds * (plan.video_kbps + plan.audio_kbps) as f64 * 1000.0 / 8.0 * 1.02;
            Some((after as u64, changed("mp4")))
        }
        Kind::Audio => {
            let seconds = av::duration(path)?;
            let (kbps, _) = av::plan_audio(level);
            let ext = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp3")) && crate::convert::media::find_ffmpeg().is_some() { "mp3" } else { "m4a" };
            Some(((seconds * kbps as f64 * 1000.0 / 8.0 * 1.02) as u64, changed(ext)))
        }
    }
}

/// The estimates' `after`s, for tests elsewhere
#[cfg(test)]
pub(super) fn tests_estimate(paths: &[PathBuf]) -> Vec<Option<u64>> {
    collect(paths, 0).into_iter().map(|e| e.after).collect()
}

/// All four estimates, Light first
#[cfg(test)]
fn collect(paths: &[PathBuf], token: u32) -> Vec<Estimate> {
    let all = Mutex::new(Vec::new());
    estimate(paths, token, &|e| all.lock().unwrap().push(e));
    let mut all = all.into_inner().unwrap();
    all.sort_by_key(|e| LEVELS.iter().position(|l| l.id() == e.id));
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `FLIPFILE_ESTIMATE=a.jpg:b.pdf cargo test --profile bench --lib estimates_files -- --ignored --nocapture`:
    /// how long each level's estimate takes to arrive
    #[test]
    #[ignore]
    fn estimates_files() {
        for path in std::env::var("FLIPFILE_ESTIMATE").unwrap_or_default().split(':').filter(|p| !p.is_empty()) {
            let started = std::time::Instant::now();
            estimate(&[PathBuf::from(path)], 0, &|e| {
                eprintln!("{path}: {} after {:.2} s: {:?} of {} ({})", e.id, started.elapsed().as_secs_f64(), e.after, e.before, e.note)
            });
        }
    }

    /// A noisy photo-like PNG: every level gets smaller than the one before, says why,
    /// and Balanced on turns it into a JPG
    #[test]
    fn photo_levels_get_smaller() {
        let dir = std::env::temp_dir().join(format!("flipfile-test-estimate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("photo.png");
        let mut seed = 7u32;
        let img = image::RgbImage::from_fn(2600, 1800, |x, y| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let noise = (seed >> 24) as u8 / 8;
            image::Rgb([((x / 7) as u8).wrapping_add(noise), ((y / 5) as u8).wrapping_add(noise), ((x + y) / 11) as u8])
        });
        img.save(&path).unwrap();
        let estimates = collect(&[path.clone()], 9);
        assert_eq!(estimates.len(), 4);
        assert!(estimates.iter().all(|e| e.token == 9 && !e.approximate));
        let afters: Vec<u64> = estimates.iter().map(|e| e.after.unwrap()).collect();
        for pair in afters.windows(2) {
            assert!(pair[1] < pair[0], "{afters:?}");
        }
        assert!(afters[0] <= estimates[0].before);
        assert_eq!(estimates[0].note, "Looks the same", "Light keeps it a PNG");
        assert!(estimates[2].note.starts_with("PNG → JPG · Fine detail softens"), "{}", estimates[2].note);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
