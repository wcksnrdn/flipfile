// Speech to text (Transcribe): whisper.cpp runs OpenAI's Whisper `small` model, bundled
// with flipfile (src-tauri/models/whisper-small.bin, fetched by scripts/fetch-models.mjs),
// on every system: on Apple silicon on the GPU through Metal, elsewhere on the CPU. The
// audio reaches it as 16 kHz mono samples (src/convert/transcribe/audio.rs), the long
// stretches without speech cut out first (Silero VAD, also bundled); it answers with
// timed words in the language it hears.

use std::borrow::Cow;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use whisper_rs::{
    DtwMode, DtwModelPreset, DtwParameters, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperVadContext, WhisperVadContextParams,
    WhisperVadParams,
};

/// The sample rate Whisper hears
pub const SAMPLE_RATE: usize = 16_000;
const MODEL: &str = "whisper-small.bin";
/// Silero VAD (MIT), in whisper.cpp's format: tells speech from silence, music and noise
const VAD_MODEL: &str = "whisper-vad.bin";
/// Text the model takes for what came before the recording. Whisper writes the way the
/// text before it is written: without it, speech over music (voice-overs, ads) often
/// comes out as one endless lowercase run with no full stop, and so does every window
/// after it; with it, sentences end where they are spoken. Measured on a 47 s voice-over:
/// none of its 13 sentences ended without it, all of them with it. It never makes the
/// model translate: Spanish, German, Indonesian, Japanese and Chinese came out in their
/// own language with it, as before.
const PROMPT: &str = "Hello, welcome. This is a transcript, with punctuation.";
/// One stretch of speech: a word as the model times it, or (src/convert/transcribe) a
/// sentence or a subtitle cue
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    /// Seconds from the start of the recording
    pub start: f64,
    pub end: f64,
    /// As the model writes it, with the space in front (" Good morning,"); a language
    /// written without spaces between words has none
    pub text: String,
    /// It ends one of the model's phrases: where the speaker paused or a thought ended,
    /// a natural place for a break when the model wrote no punctuation
    pub phrase_end: bool,
}

pub struct Transcript {
    /// The language heard, in English ("Indonesian"), when the model could tell
    pub language: Option<String>,
    /// The words heard, each with its time, the last of each of the model's phrases
    /// (where the speaker paused or a sentence ended) marked. A language written without
    /// spaces between words has a phrase where the others have a word.
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
    // Every word placed where it is heard: dynamic time warping over the attention of
    // the heads that follow the audio, for `small`. The model's own timestamps put a word
    // seconds early after a pause; these are within a third of a second. It costs flash
    // attention, which it cannot work with: 226 s of speech took 17 s instead of 11 s on
    // an M1 Pro's GPU, 27 s instead of 22 s on its CPU (where flash attention is slower
    // anyway).
    params.flash_attn(false);
    params.dtw_parameters(DtwParameters { mode: DtwMode::ModelPreset { model_preset: DtwModelPreset::Small }, ..Default::default() });
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
    let speech = Speech::of(pcm);
    if speech.pieces.is_empty() {
        on_progress(100, 100);
        return Ok(Transcript { language: None, segments: vec![] });
    }

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
            params.set_initial_prompt(PROMPT);
            params.set_token_timestamps(true);
            params.set_suppress_blank(true);
            // No "[MUSIC]" or "♪" tokens
            params.set_suppress_nst(true);
            params.set_progress_callback_safe(move |percent: i32| {
                let _ = tx.send(percent);
            });
            state.full(params, &speech.pcm).map_err(|e| format!("the speech model failed ({e})"))?;
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
    let special = context.token_eot();
    let mut segments = vec![];
    for segment in state.as_iter() {
        let Ok(text) = segment.to_str_lossy() else { continue };
        let phrase = text.trim();
        // Whisper describes sounds in brackets ("[tertawa]", "(music)") now and then
        let bracketed = phrase.starts_with(['[', '(']) && phrase.ends_with([']', ')']);
        if phrase.is_empty() || bracketed {
            continue;
        }
        let start = segment.start_timestamp() as f64 / 100.0;
        let end = segment.end_timestamp() as f64 / 100.0;
        // The tokens of the phrase (pieces of words), each with the time it is heard at
        let tokens: Vec<(Vec<u8>, f64)> = (0..segment.n_tokens())
            .filter_map(|i| {
                let token = segment.get_token(i)?;
                let data = token.token_data();
                let heard = if data.t_dtw >= 0 { data.t_dtw } else { data.t0 };
                // The timestamps and markers among them are not text
                (data.id < special).then(|| (token.to_bytes().unwrap_or_default().to_vec(), heard as f64 / 100.0))
            })
            .collect();
        let mut words = timed_words(&tokens, end);
        // Should the pieces not add up to the phrase, the phrase stays whole
        if words.iter().map(|w| w.text.as_str()).collect::<String>().trim_end() != text.trim_end() {
            words = vec![Segment { start, end, text: text.trim_end().to_string(), phrase_end: true }];
        }
        // Back to the recording's own time
        segments.extend(words.into_iter().map(|w| Segment { start: speech.time(w.start), end: speech.time(w.end), ..w }));
    }
    Ok(Transcript { language, segments })
}

/// The recording with its long stretches without speech (silence, music, noise) cut down
/// to a short silence, and the way back to the recording's own time. Whisper writes
/// something for whatever it hears: given a minute of music or a quiet room, it makes up
/// a sentence ("Thank you for watching."), or repeats the one before over and over.
struct Speech<'a> {
    /// What Whisper hears
    pcm: Cow<'a, [f32]>,
    /// The stretches of the recording kept, in order
    pieces: Vec<Piece>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Piece {
    /// Where it starts in `pcm`, in seconds
    at: f64,
    /// Where it starts in the recording, in seconds
    from: f64,
    length: f64,
}

/// A pause shorter than this stays as it was: speech has pauses, and the model reads
/// the ends of sentences from them (seconds)
const KEEP_PAUSE: f64 = 2.0;
/// Kept around each stretch of speech, so that no word is cut off (seconds)
const SPEECH_PAD: f64 = 0.3;
/// The silence a longer stretch without speech becomes (seconds)
const CUT_TO: f64 = 0.5;

impl<'a> Speech<'a> {
    /// `pcm` without its long stretches without speech: none of it when nobody speaks,
    /// all of it when the speech detector is missing
    fn of(pcm: &'a [f32]) -> Self {
        let whole = |pcm: &'a [f32]| Speech { pieces: vec![Piece { at: 0.0, from: 0.0, length: pcm.len() as f64 / SAMPLE_RATE as f64 }], pcm: Cow::Borrowed(pcm) };
        match detect_speech(pcm) {
            Some(spoken) => Self::keeping(pcm, &spoken),
            None => whole(pcm),
        }
    }

    /// `pcm` with only the stretches `spoken` (seconds, in order), each padded, those
    /// close together joined with the pause between them
    fn keeping(pcm: &'a [f32], spoken: &[(f64, f64)]) -> Self {
        let rate = SAMPLE_RATE as f64;
        let seconds = pcm.len() as f64 / rate;
        let mut kept: Vec<(f64, f64)> = vec![];
        for &(start, end) in spoken {
            let (start, end) = ((start - SPEECH_PAD).max(0.0), (end + SPEECH_PAD).min(seconds));
            match kept.last_mut() {
                Some(last) if start - last.1 < KEEP_PAUSE => last.1 = last.1.max(end),
                _ if end > start => kept.push((start, end)),
                _ => {}
            }
        }
        // Next to nothing to cut: the recording as it is, without a copy
        let cut: f64 = seconds - kept.iter().map(|(s, e)| e - s).sum::<f64>();
        if kept.len() == 1 && cut < 1.0 {
            return Speech { pieces: vec![Piece { at: 0.0, from: 0.0, length: seconds }], pcm: Cow::Borrowed(pcm) };
        }
        let mut out = Vec::with_capacity(pcm.len());
        let mut pieces = vec![];
        for (i, (start, end)) in kept.into_iter().enumerate() {
            if i > 0 {
                out.resize(out.len() + (CUT_TO * rate) as usize, 0.0);
            }
            let (a, b) = ((start * rate) as usize, ((end * rate) as usize).min(pcm.len()));
            pieces.push(Piece { at: out.len() as f64 / rate, from: a as f64 / rate, length: (b - a) as f64 / rate });
            out.extend_from_slice(&pcm[a..b]);
        }
        Speech { pcm: Cow::Owned(out), pieces }
    }

    /// The time in the recording of a time in `pcm`: within a piece, as it was; within
    /// a cut, the nearer end of it
    fn time(&self, t: f64) -> f64 {
        let i = self.pieces.partition_point(|p| p.at <= t).saturating_sub(1);
        let Some(piece) = self.pieces.get(i) else { return t };
        let into = t - piece.at;
        if into <= piece.length {
            return piece.from + into.max(0.0);
        }
        match self.pieces.get(i + 1) {
            Some(next) if into - piece.length > CUT_TO / 2.0 => next.from,
            _ => piece.from + piece.length,
        }
    }
}

/// The stretches of `pcm` where someone speaks (seconds), as Silero VAD hears them;
/// None when it cannot run
fn detect_speech(pcm: &[f32]) -> Option<Vec<(f64, f64)>> {
    let path = super::upscale::model_path(VAD_MODEL)?;
    let mut context_params = WhisperVadContextParams::new();
    context_params.set_n_threads(threads());
    let mut vad = WhisperVadContext::new(path.to_str()?, context_params).ok()?;
    let segments = vad.segments_from_samples(WhisperVadParams::new(), pcm).ok()?;
    Some(
        (0..segments.num_segments())
            .filter_map(|i| Some((segments.get_segment_start_timestamp(i)? as f64 / 100.0, segments.get_segment_end_timestamp(i)? as f64 / 100.0)))
            .collect(),
    )
}

/// The alignment hears a word about a third of a second after it starts (measured
/// against the sound itself); a subtitle had better come a moment early than late
const HEARD_LATE: f64 = 0.2;
/// How long a word's last token lasts at most, after the time it is heard at (seconds)
const LAST_TOKEN: f64 = 0.5;

/// The words of a phrase from its tokens (bytes, time heard), the phrase ending at `end`
/// as the model has it: a word begins at a token with a space in front, and lasts until
/// the next one starts, unless a pause comes first. Times never run backwards.
fn timed_words(tokens: &[(Vec<u8>, f64)], end: f64) -> Vec<Segment> {
    // Each word with the times its first and last tokens are heard at. Punctuation is
    // not heard: the alignment puts a full stop just before whatever is said next, after
    // the pause.
    let mut words: Vec<(Vec<u8>, f64, f64)> = vec![];
    for (bytes, heard) in tokens {
        match words.last_mut() {
            Some(word) if !bytes.starts_with(b" ") => {
                word.0.extend_from_slice(bytes);
                if !String::from_utf8_lossy(bytes).chars().all(|c| c.is_whitespace() || c.is_ascii_punctuation() || "。，、！？；：…「」『』（）—–“”‘’«»¿¡".contains(c)) {
                    word.2 = word.2.max(*heard);
                }
            }
            _ => words.push((bytes.clone(), *heard, *heard)),
        }
    }
    let mut starts: Vec<f64> = Vec::with_capacity(words.len());
    for (_, first, _) in &words {
        let earliest = starts.last().copied().unwrap_or(0.0);
        starts.push((first - HEARD_LATE).max(earliest));
    }
    let count = words.len();
    words
        .into_iter()
        .enumerate()
        .map(|(i, (bytes, _, heard_last))| {
            let start = starts[i];
            let spoken = heard_last.max(start);
            let until = match starts.get(i + 1) {
                Some(&next) => next.min(spoken + LAST_TOKEN),
                // The phrase's end, unless the model has it before the word or long after
                None if end > spoken => end.min(spoken + LAST_TOKEN),
                None => spoken + HEARD_LATE,
            };
            let last = i + 1 == count;
            let text = String::from_utf8_lossy(&bytes);
            Segment { start, end: until.max(start), text: if last { text.trim_end().to_string() } else { text.into_owned() }, phrase_end: last }
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn token(text: &str, heard: f64) -> (Vec<u8>, f64) {
        (text.as_bytes().to_vec(), heard)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn words_from_tokens() {
        let tokens = [token(" Need", 1.0), token(" sub", 1.3), token("titles", 1.5), token(" for", 1.7), token(" you", 1.8), token("?", 3.9), token(" Ten", 4.0), token(" free.", 4.3)];
        let words = timed_words(&tokens, 4.6);
        let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, [" Need", " subtitles", " for", " you?", " Ten", " free."]);
        // A moment before it is heard, until the next word
        assert!(close(words[0].start, 0.8) && close(words[0].end, 1.1), "{words:?}");
        assert!(close(words[1].start, 1.1) && close(words[1].end, 1.5), "{words:?}");
        // Before a pause: no longer than its last token lasts, its question mark aside
        assert_eq!(words[3].text, " you?");
        assert!(close(words[3].end, 2.3), "{words:?}");
        assert!(close(words[4].start, 3.8), "{words:?}");
        assert!(close(words[5].end, 4.6), "the last word ends with the phrase");
        assert_eq!(words.iter().filter(|w| w.phrase_end).count(), 1);
        assert!(words[5].phrase_end);
        // A phrase the model ends long after its last word, or before it
        assert!(close(timed_words(&tokens, 30.0)[5].end, 4.8));
        assert!(close(timed_words(&tokens, 2.0)[5].end, 4.5));
    }

    /// Long stretches without speech shrink to a short silence, short pauses stay, and
    /// times come back to the recording's own
    #[test]
    fn speech_keeps_pauses_and_cuts_the_rest() {
        let pcm: Vec<f32> = (0..60 * SAMPLE_RATE).map(|i| (i % 100) as f32 / 100.0).collect();
        // Music until 10 s, speech with a 1 s pause, a quiet minute's end, speech again
        let speech = Speech::keeping(&pcm, &[(10.0, 14.0), (15.0, 20.0), (50.0, 55.0)]);
        assert_eq!(speech.pieces.len(), 2, "{:?}", speech.pieces);
        let [first, second] = [speech.pieces[0], speech.pieces[1]];
        assert!((first.from - 9.7).abs() < 1e-3 && (first.length - 10.6).abs() < 1e-3, "{first:?}");
        assert!((second.at - (first.length + CUT_TO)).abs() < 1e-3 && (second.from - 49.7).abs() < 1e-3, "{second:?}");
        assert_eq!(speech.pcm.len(), ((first.length + CUT_TO + second.length) * SAMPLE_RATE as f64).round() as usize);
        // The kept audio is the recording's own
        let a = (second.at * SAMPLE_RATE as f64) as usize;
        assert_eq!(speech.pcm[a..a + 100], pcm[(49.7 * SAMPLE_RATE as f64) as usize..][..100]);

        assert!((speech.time(0.0) - 9.7).abs() < 1e-3);
        assert!((speech.time(5.3) - 15.0).abs() < 1e-3, "the pause kept");
        // Within the cut: its nearer end
        assert!((speech.time(first.length + 0.1) - 20.3).abs() < 1e-3);
        assert!((speech.time(first.length + 0.4) - 49.7).abs() < 1e-3);
        assert!((speech.time(second.at + 2.0) - 51.7).abs() < 1e-3);
        assert!((speech.time(1e9) - 55.3).abs() < 1e-3, "past the end: the end");
    }

    #[test]
    fn speech_of_nothing_and_everything() {
        let pcm = vec![0.0; 30 * SAMPLE_RATE];
        assert!(Speech::keeping(&pcm, &[]).pieces.is_empty());
        let all = Speech::keeping(&pcm, &[(0.2, 12.0), (12.5, 29.8)]);
        assert!(matches!(all.pcm, Cow::Borrowed(_)) && all.pieces.len() == 1);
        assert_eq!(all.time(7.5), 7.5);
    }

    /// A character split across tokens comes out whole; times out of order never run
    /// backwards
    #[test]
    fn tokens_mend_characters_and_times() {
        let (a, b) = "好".as_bytes().split_at(1);
        let tokens = [token("大家早上", 5.0), (a.to_vec(), 5.4), (b.to_vec(), 5.6)];
        let words = timed_words(&tokens, 7.0);
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].text, "大家早上好");
        assert!(close(words[0].start, 4.8) && close(words[0].end, 6.1), "{words:?}");

        let tokens = [token(" one", 1.0), token(" two", 0.5), token(" three", 9.0)];
        let words = timed_words(&tokens, 9.2);
        for pair in words.windows(2) {
            assert!(pair[1].start >= pair[0].start && pair[0].end <= pair[1].start + 1e-9, "{words:?}");
        }
        assert!(words.iter().all(|w| w.end >= w.start), "{words:?}");
    }
}
