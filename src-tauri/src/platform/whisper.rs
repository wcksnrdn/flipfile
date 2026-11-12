// Speech to text (Transcribe): whisper.cpp runs OpenAI's Whisper `small` model, bundled
// with flipfile (src-tauri/models/whisper-small.bin, fetched by scripts/fetch-models.mjs),
// on every system: on Apple silicon on the GPU through Metal, elsewhere on the CPU. The
// audio reaches it as 16 kHz mono samples (src/convert/transcribe/audio.rs); it answers
// with timed segments of text in the language it hears.

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// The sample rate Whisper hears
pub const SAMPLE_RATE: usize = 16_000;
const MODEL: &str = "whisper-small.bin";
/// One stretch of speech: a phrase as the model hears it, or (src/convert/transcribe)
/// a word, a sentence or a subtitle cue
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    /// Seconds from the start of the recording
    pub start: f64,
    pub end: f64,
    /// As the model writes it, with the space in front (" Good morning,"); a language
    /// written without spaces between words has none
    pub text: String,
}

pub struct Transcript {
    /// The language heard, in English ("Indonesian"), when the model could tell
    pub language: Option<String>,
    /// The phrases heard, as the model cut them (where the speaker paused or a
    /// sentence ended), with their timing
    pub segments: Vec<Segment>,
}

/// Whether the model runs on a GPU here (Apple silicon, through Metal)
const GPU: bool = cfg!(all(target_os = "macos", target_arch = "aarch64"));

/// The loaded model: loading takes under a second, and a loaded model holds about
/// half a gigabyte, so it is let go of after every batch (`release`). One recording at a
/// time: a transcription uses every core (or the GPU) by itself.
static CONTEXT: Mutex<Option<WhisperContext>> = Mutex::new(None);

/// True when the model is bundled and this processor can run it
#[cfg(test)]
pub fn available() -> bool {
    super::upscale::model_path(MODEL).is_some() && processor_supported()
}

/// x86-64 builds assume AVX2, FMA and F16C (.cargo/config.toml): without them the engine
/// would stop the app with an illegal instruction instead of failing politely
fn processor_supported() -> bool {
    #[cfg(target_arch = "x86_64")]
    return std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") && std::arch::is_x86_feature_detected!("f16c");
    #[cfg(not(target_arch = "x86_64"))]
    true
}

fn quiet() {
    // whisper.cpp reports every step on stderr unless told otherwise
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(whisper_rs::install_logging_hooks);
}

fn load() -> Result<WhisperContext, String> {
    if !processor_supported() {
        return Err("transcribing needs a newer processor (one with AVX2, made in 2013 or later)".into());
    }
    quiet();
    #[cfg(target_os = "macos")]
    without_residency_sets();
    let path = super::upscale::model_path(MODEL).ok_or("The speech model is missing. Please reinstall flipfile.")?;
    let mut params = WhisperContextParameters::default();
    // Quicker on the GPU, slower on the CPU (163 s instead of 96 s for 109 s of speech
    // on an M1 Pro's CPU)
    params.flash_attn(GPU);
    WhisperContext::new_with_params(&path, params).map_err(|e| format!("the speech model could not be loaded ({e})"))
}

/// Writes down what is said in `pcm` (16 kHz mono, -1 to 1). `on_progress` hears
/// (percent done, 100) now and then.
pub fn transcribe(pcm: &[f32], on_progress: &(dyn Fn(usize, usize) + Sync)) -> Result<Transcript, String> {
    let mut context = CONTEXT.lock().unwrap_or_else(|e| e.into_inner());
    if context.is_none() {
        *context = Some(load()?);
    }
    let context = context.as_ref().expect("loaded above");
    let mut state = context.create_state().map_err(|e| format!("the speech model could not start ({e})"))?;

    // The progress callback runs on the transcribing thread, which is busy until the end,
    // so transcription runs on a thread of its own and this one passes the progress on
    let (tx, rx) = std::sync::mpsc::channel::<i32>();
    let result = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            // A GPU has time for a beam search of five candidates, slightly more accurate;
            // a CPU takes the likeliest word each time, twice as quick
            let strategy = if GPU { SamplingStrategy::BeamSearch { beam_size: 5, patience: -1.0 } } else { SamplingStrategy::Greedy { best_of: 1 } };
            let mut params = FullParams::new(strategy);
            params.set_language(Some("auto"));
            params.set_n_threads(threads());
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);
            params.set_print_special(false);
            // The model's own phrases, not words timed one by one: that costs 45% more
            // time on a CPU, and the phrases already end where the speaker paused
            params.set_suppress_blank(true);
            // No "[MUSIC]" or "♪" tokens
            params.set_suppress_nst(true);
            params.set_progress_callback_safe(move |percent: i32| {
                let _ = tx.send(percent);
            });
            state.full(params, pcm).map_err(|e| format!("the speech model failed ({e})"))?;
            Ok::<_, String>(state)
        });
        loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(percent) => on_progress(percent.clamp(0, 100) as usize, 100),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) if worker.is_finished() => break,
                Err(_) => {}
            }
        }
        worker.join().unwrap_or_else(|_| Err("the speech model failed (internal error)".into()))
    });
    let state = result?;
    on_progress(100, 100);

    let language = whisper_rs::get_lang_str_full(state.full_lang_id_from_state()).map(capitalised);
    let segments = state
        .as_iter()
        .filter_map(|segment| {
            let text = segment.to_str_lossy().ok()?.to_string();
            let phrase = text.trim();
            // Whisper describes sounds in brackets ("[tertawa]", "(music)") now and then
            let bracketed = phrase.starts_with(['[', '(']) && phrase.ends_with([']', ')']);
            if phrase.is_empty() || bracketed {
                return None;
            }
            Some(Segment {
                start: segment.start_timestamp() as f64 / 100.0,
                end: segment.end_timestamp() as f64 / 100.0,
                text: text.trim_end().to_string(),
            })
        })
        .collect();
    Ok(Transcript { language, segments })
}

/// ggml's Metal backend keeps the model's GPU memory in "residency sets", and checks
/// that they are empty when the process ends: a model still loaded then (the app quit
/// during a transcription) aborts the process, a crash report on every such quit. Without
/// them a transcription is just as quick (measured on an M1 Pro), and quitting is safe.
#[cfg(target_os = "macos")]
fn without_residency_sets() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        if std::env::var_os("GGML_METAL_NO_RESIDENCY").is_none() {
            // SAFETY: macOS' libc guards the environment with a lock, so setting a
            // variable while other threads read theirs is sound there; ggml reads this
            // one when the model loads, right after.
            unsafe { std::env::set_var("GGML_METAL_NO_RESIDENCY", "1") };
        }
    });
}

/// Lets go of the model and the memory it holds (it loads again in under a second).
/// A model in use is left alone: the app quitting mid-transcription must not wait for
/// it, and whoever uses it lets go of it at the end of their batch.
pub fn release() {
    match CONTEXT.try_lock() {
        Ok(mut context) => *context = None,
        Err(std::sync::TryLockError::Poisoned(e)) => *e.into_inner() = None,
        Err(std::sync::TryLockError::WouldBlock) => {}
    }
}

/// Whisper scales up to about eight threads; more only contend
fn threads() -> i32 {
    std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8) as i32
}

fn capitalised(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
