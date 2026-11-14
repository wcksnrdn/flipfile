// Transcribe (Music > Transcribe, Video > Tools): the speech in a recording or a video
// written down by Whisper (src/platform/whisper.rs), as subtitles (SRT, VTT) or as a
// transcript (TXT, Word, Markdown), in the language spoken.
//
// - audio: the file's sound as the 16 kHz mono samples the model hears

pub mod audio;

use std::fs;
use std::path::Path;

use super::input::file_name;
use super::office_extra::{blocks_to_docx, DocBlock};
use super::output::{save_err, sibling_output_path, unique_output_path};
use super::{Converted, OnPage};
use crate::platform::whisper::{self, Segment};

/// True for the Transcribe targets, which report their progress in steps (percent)
pub(super) fn is_transcribe_target(format: &str) -> bool {
    matches!(format, "tr-srt" | "tr-vtt" | "tr-txt" | "tr-docx" | "tr-md")
}

/// A recording or a video, by its name: what an AI summary transcribes before reading
pub fn is_recording(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    matches!(
        ext.as_str(),
        "mp3" | "wav" | "m4a" | "aac" | "flac" | "ogg" | "oga" | "opus" | "aiff" | "aif" | "caf" | "wma" | "amr" | "m4b" | "weba"
            | "mp4" | "mov" | "m4v" | "mkv" | "webm" | "avi" | "flv" | "wmv" | "mpeg" | "mpg" | "ts" | "3gp" | "ogv"
    )
}

/// Writes down what is said in `input`. `on_step` hears (percent, 100).
pub(super) fn transcribe_file(input: &Path, format: &str, out_dir: Option<&Path>, on_step: OnPage) -> Result<Converted, String> {
    let name = file_name(input);
    on_step(0, 100);
    let pcm = audio::read(input)?;
    let seconds = pcm.len() as f64 / whisper::SAMPLE_RATE as f64;
    let transcript = whisper::transcribe(&pcm, &on_step)?;
    drop(pcm);
    let words = words(&transcript.segments);
    if words.is_empty() {
        return Err(format!("\"{name}\": no speech was heard"));
    }
    let title = input.file_stem().and_then(|s| s.to_str()).unwrap_or("Transcript");
    let sentences = sentences(&words);

    let (ext, bytes) = match format {
        "tr-srt" => ("srt", srt(&cues(&words)).into_bytes()),
        "tr-vtt" => ("vtt", vtt(&cues(&words)).into_bytes()),
        "tr-txt" => ("txt", plain(&sentences).into_bytes()),
        "tr-md" => ("md", markdown(title, transcript.language.as_deref(), &sentences, seconds).into_bytes()),
        "tr-docx" => {
            let blocks = blocks(title, transcript.language.as_deref(), &sentences, seconds);
            ("docx", blocks_to_docx(&blocks).map_err(|e| format!("\"{name}\": {e}"))?)
        }
        _ => return Err(format!("\"{name}\": cannot transcribe to {format}")),
    };
    // Subtitles take the recording's own name, which is how players find them next to a
    // video (`lecture.mp4` + `lecture.srt`); transcripts say what they are
    let output = match format {
        "tr-srt" | "tr-vtt" => sibling_output_path(input, out_dir, ext),
        _ => unique_output_path(input, out_dir, "transcript", ext),
    };
    fs::write(&output, bytes).map_err(save_err)?;
    let note = match &transcript.language {
        Some(language) => format!("{language} · {}", clock(seconds)),
        None => clock(seconds),
    };
    Ok(Converted { path: output.to_string_lossy().into_owned(), pages: 0, sizes: None, note: Some(note) })
}

/// The transcript as plain text, for AI summaries of a recording (src/convert/text.rs):
/// the paragraphs, and the language heard
pub fn transcript_text(input: &Path, on_step: OnPage) -> Result<(String, Option<String>), String> {
    let name = file_name(input);
    on_step(0, 100);
    let pcm = audio::read(input)?;
    let transcript = whisper::transcribe(&pcm, &on_step);
    // The summary's own model needs the memory next
    whisper::release();
    let transcript = transcript?;
    let words = words(&transcript.segments);
    if words.is_empty() {
        return Err(format!("\"{name}\": no speech was heard"));
    }
    Ok((plain(&sentences(&words)), transcript.language))
}

/// Characters on a subtitle line, and lines in a cue (the usual broadcast limits)
const LINE: usize = 42;
const CUE_CHARS: usize = 2 * LINE;
/// The longest a cue stays on screen, in seconds
const CUE_SECONDS: f64 = 7.0;
/// A pause this long ends a sentence even without a full stop (seconds)
const SENTENCE_PAUSE: f64 = 1.2;
/// Without a full stop, a sentence ends at the end of the model's phrase once it is this
/// long (seconds): speech the model wrote without any punctuation (voice-overs over music
/// often come out that way) would otherwise be one endless sentence
const LONG_SENTENCE: f64 = 6.0;
/// A paragraph of the transcript spans at most this long (seconds): two or three
/// sentences, so that a timestamp is never far from what it points at
const PARAGRAPH_SECONDS: f64 = 12.0;

/// The words of the model's phrases, each with its time. The model times its words
/// itself; a phrase that comes whole has its time shared out by its words' lengths
/// (speech runs at a fairly even number of syllables a second). A language written
/// without spaces keeps each phrase whole.
pub fn words(segments: &[Segment]) -> Vec<Segment> {
    let mut words = vec![];
    for segment in segments {
        // Each word with the space in front of it
        let mut pieces: Vec<String> = vec![];
        for (i, piece) in segment.text.split(' ').enumerate() {
            if piece.is_empty() {
                continue;
            }
            let lead = if i > 0 || segment.text.starts_with(' ') { " " } else { "" };
            pieces.push(format!("{lead}{piece}"));
        }
        let total: usize = pieces.iter().map(|p| p.trim().chars().count() + 1).sum();
        let span = (segment.end - segment.start).max(0.0);
        let mut at = 0usize;
        let count = pieces.len();
        for (i, piece) in pieces.into_iter().enumerate() {
            let length = piece.trim().chars().count() + 1;
            let start = segment.start + span * at as f64 / total as f64;
            at += length;
            let last = i + 1 == count;
            let end = if last { segment.end } else { segment.start + span * at as f64 / total as f64 };
            words.push(Segment { start, end, text: piece, phrase_end: last && segment.phrase_end });
        }
    }
    words
}

fn ends_sentence(word: &str) -> bool {
    word.trim_end_matches(['"', '\'', '”', '’', ')', '»']).ends_with(['.', '!', '?', '…', '。', '！', '？'])
}

fn ends_clause(word: &str) -> bool {
    word.ends_with([',', ';', ':', '，', '、', '；', '：']) || word.ends_with(" -") || word.ends_with('–')
}

/// Words put together into one stretch of text
fn join(words: &[Segment]) -> Segment {
    Segment {
        start: words.first().map_or(0.0, |w| w.start),
        end: words.last().map_or(0.0, |w| w.end),
        text: words.iter().map(|w| w.text.as_str()).collect::<String>().trim().to_string(),
        phrase_end: words.last().is_some_and(|w| w.phrase_end),
    }
}

/// Where the sentences end: after a full stop, a question or exclamation mark, a long
/// pause, or (without punctuation) the end of a long enough phrase of the model's. As
/// indices into `words`, each the end of a sentence (exclusive).
fn sentence_ends(words: &[Segment]) -> Vec<usize> {
    let mut ends = vec![];
    let mut start = words.first().map_or(0.0, |w| w.start);
    for i in 0..words.len() {
        let last = i + 1 == words.len();
        let pause = !last && words[i + 1].start - words[i].end >= SENTENCE_PAUSE;
        let long = words[i].phrase_end && words[i].end - start >= LONG_SENTENCE;
        if last || pause || long || ends_sentence(&words[i].text) {
            ends.push(i + 1);
            if !last {
                start = words[i + 1].start;
            }
        }
    }
    ends
}

/// The words gathered into sentences
pub fn sentences(words: &[Segment]) -> Vec<Segment> {
    let mut from = 0;
    sentence_ends(words)
        .into_iter()
        .map(|end| {
            let sentence = join(&words[from..end]);
            from = end;
            sentence
        })
        .collect()
}

/// Subtitle cues: every sentence in a cue of its own, a sentence too long for one
/// (two lines, seven seconds) cut into as few as needed, preferably after a comma or at
/// the end of one of the model's phrases, else where the pieces come out even. Never shorter than a moment, never overlapping.
pub fn cues(words: &[Segment]) -> Vec<Segment> {
    let mut cues = vec![];
    let mut from = 0;
    for end in sentence_ends(words) {
        split_cue(&words[from..end], &mut cues);
        from = end;
    }
    for i in 0..cues.len() {
        let next = cues.get(i + 1).map(|c: &Segment| c.start);
        let cue = &mut cues[i];
        if cue.end < cue.start + 0.7 {
            cue.end = cue.start + 0.7;
        }
        if let Some(next) = next {
            if cue.end > next {
                cue.end = next.max(cue.start);
            }
        }
    }
    cues
}

fn split_cue(words: &[Segment], out: &mut Vec<Segment>) {
    if words.is_empty() {
        return;
    }
    let cue = join(words);
    let fits = cue.text.chars().count() <= CUE_CHARS && cue.end - cue.start <= CUE_SECONDS;
    if fits || words.len() == 1 {
        out.push(cue);
        return;
    }
    let total = cue.text.chars().count() as f64;
    let best = (1..words.len())
        .map(|at| {
            let left = join(&words[..at]).text.chars().count() as f64;
            let uneven = (2.0 * left - total).abs() / total;
            let natural = ends_clause(words[at - 1].text.trim_end()) || words[at - 1].phrase_end;
            let clause = if natural { 0.0 } else { 0.35 };
            (at, uneven + clause)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(words.len() / 2, |(at, _)| at);
    split_cue(&words[..best], out);
    split_cue(&words[best..], out);
}

pub fn srt(cues: &[Segment]) -> String {
    let mut out = String::new();
    for (i, cue) in cues.iter().enumerate() {
        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, stamp(cue.start, ','), stamp(cue.end, ','), wrap(&cue.text)));
    }
    out
}

pub fn vtt(cues: &[Segment]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for cue in cues {
        out.push_str(&format!("{} --> {}\n{}\n\n", stamp(cue.start, '.'), stamp(cue.end, '.'), wrap(&cue.text)));
    }
    out
}

/// `HH:MM:SS,mmm` (SRT) or `HH:MM:SS.mmm` (VTT)
fn stamp(seconds: f64, separator: char) -> String {
    let ms = (seconds.max(0.0) * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02}{separator}{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// `M:SS` or `H:MM:SS`, for people: rounded down, so that seeking to a paragraph's
/// timestamp starts just before its first word
fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// A cue of more than one line's worth is broken in two at the word nearest its middle
/// (after a comma, if one is close to it)
fn wrap(text: &str) -> String {
    if text.chars().count() <= LINE {
        return text.to_string();
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 2 {
        return text.to_string();
    }
    let total = text.chars().count();
    let mut best = (usize::MAX, 0);
    let mut length = 0;
    for (i, word) in words.iter().enumerate().take(words.len() - 1) {
        length += word.chars().count() + if i > 0 { 1 } else { 0 };
        let rest = total - length - 1;
        // A comma within a few characters of the middle is the better place
        let unevenness = length.abs_diff(rest) * 2 + if ends_clause(word) { 0 } else { 8 };
        if length <= LINE + 4 && rest <= LINE + 4 && unevenness < best.0 {
            best = (unevenness, i + 1);
        }
    }
    if best.1 == 0 {
        // Neither line fits whatever the break: the most even one
        best.1 = words.len() / 2;
    }
    format!("{}\n{}", words[..best.1].join(" "), words[best.1..].join(" "))
}

/// Sentences gathered into paragraphs: a new one starts after a pause, or when the
/// paragraph would grow longer than `PARAGRAPH_SECONDS` or too long to read
fn paragraphs(sentences: &[Segment]) -> Vec<Segment> {
    const PAUSE: f64 = 1.5;
    const FULL: usize = 700;
    let mut out: Vec<Segment> = Vec::new();
    for sentence in sentences {
        match out.last_mut() {
            Some(last)
                if sentence.start - last.end < PAUSE
                    && sentence.end - last.start <= PARAGRAPH_SECONDS
                    && last.text.len() < FULL =>
            {
                last.text.push(' ');
                last.text.push_str(&sentence.text);
                last.end = sentence.end;
            }
            _ => out.push(sentence.clone()),
        }
    }
    out
}

pub fn plain(sentences: &[Segment]) -> String {
    paragraphs(sentences).iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join("\n\n") + "\n"
}

fn subtitle(language: Option<&str>, seconds: f64) -> String {
    match language {
        Some(language) => format!("Transcript · {language} · {}", clock(seconds)),
        None => format!("Transcript · {}", clock(seconds)),
    }
}

fn markdown(title: &str, language: Option<&str>, sentences: &[Segment], seconds: f64) -> String {
    let mut out = format!("# {title}\n\n_{}_\n\n", subtitle(language, seconds));
    for paragraph in paragraphs(sentences) {
        out.push_str(&format!("**[{}]** {}\n\n", clock(paragraph.start), paragraph.text));
    }
    out
}

fn blocks(title: &str, language: Option<&str>, sentences: &[Segment], seconds: f64) -> Vec<DocBlock> {
    let mut blocks = vec![DocBlock::Heading(1, title.to_string()), DocBlock::Paragraph(subtitle(language, seconds))];
    blocks.extend(paragraphs(sentences).into_iter().map(|p| DocBlock::Paragraph(format!("[{}] {}", clock(p.start), p.text))));
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The words of `text` spread evenly from `start` to `end`, as the model times them
    fn timed(text: &str, start: f64, end: f64) -> Vec<Segment> {
        let list: Vec<&str> = text.split_whitespace().collect();
        let step = (end - start) / list.len() as f64;
        list.iter()
            .enumerate()
            .map(|(i, w)| Segment {
                start: start + i as f64 * step,
                end: start + (i + 1) as f64 * step,
                text: format!(" {w}"),
                phrase_end: i + 1 == list.len(),
            })
            .collect()
    }

    fn spoken(parts: &[(&str, f64, f64)]) -> Vec<Segment> {
        parts.iter().flat_map(|(text, start, end)| timed(text, *start, *end)).collect()
    }

    #[test]
    fn stamps_and_clocks() {
        assert_eq!(stamp(0.0, ','), "00:00:00,000");
        assert_eq!(stamp(3723.4567, ','), "01:02:03,457");
        assert_eq!(stamp(59.9999, '.'), "00:01:00.000");
        assert_eq!(clock(754.0), "12:34");
        assert_eq!(clock(3661.0), "1:01:01");
    }

    #[test]
    fn a_cue_per_sentence() {
        let words = spoken(&[("Selamat pagi semuanya.", 0.0, 2.5), ("Ya.", 2.5, 2.6), ("Terima kasih.", 5.0, 6.0)]);
        let srt = srt(&cues(&words));
        let expected = "1\n00:00:00,000 --> 00:00:02,500\nSelamat pagi semuanya.\n\n2\n00:00:02,500 --> 00:00:03,200\nYa.\n\n3\n00:00:05,000 --> 00:00:06,000\nTerima kasih.\n\n";
        assert_eq!(srt, expected);
        assert!(vtt(&cues(&words)).starts_with("WEBVTT\n\n00:00:00.000 --> 00:00:02.500\nSelamat pagi semuanya.\n"));
    }

    /// A long sentence is cut after its comma, not mid-clause, and no cue outgrows two lines
    #[test]
    fn long_sentences_are_cut_at_commas() {
        let text = "Hari ini kita akan membahas transkripsi otomatis di komputer sendiri, tanpa mengirim apa pun ke internet sama sekali.";
        let cues = cues(&timed(text, 0.0, 8.0));
        assert_eq!(cues.len(), 2, "{cues:?}");
        assert_eq!(cues[0].text, "Hari ini kita akan membahas transkripsi otomatis di komputer sendiri,");
        assert_eq!(cues[1].text, "tanpa mengirim apa pun ke internet sama sekali.");
        assert!(cues[0].end <= cues[1].start);
        // A run-on without punctuation is cut evenly, every cue within the limits
        let run_on = "satu dua tiga empat lima enam tujuh delapan sembilan sepuluh sebelas dua belas tiga belas empat belas lima belas enam belas tujuh belas";
        for cue in super::cues(&timed(run_on, 0.0, 20.0)) {
            assert!(cue.text.chars().count() <= CUE_CHARS && cue.end - cue.start <= CUE_SECONDS + 1e-9, "{cue:?}");
        }
    }

    #[test]
    fn sentences_end_at_stops_and_pauses() {
        let words = spoken(&[("Pertama. Kedua tanpa titik", 0.0, 3.0), ("lalu ketiga!", 5.0, 6.0)]);
        let s: Vec<String> = sentences(&words).into_iter().map(|s| s.text).collect();
        assert_eq!(s, ["Pertama.", "Kedua tanpa titik", "lalu ketiga!"]);
    }

    #[test]
    fn paragraphs_follow_pauses() {
        let words = spoken(&[
            ("Pertama. Masih paragraf yang sama.", 0.0, 4.0),
            ("Kedua, setelah jeda.", 8.0, 9.0),
            ("Ketiga, setelah jeda panjang", 14.0, 15.0),
        ]);
        let sentences = sentences(&words);
        assert_eq!(plain(&sentences), "Pertama. Masih paragraf yang sama.\n\nKedua, setelah jeda.\n\nKetiga, setelah jeda panjang\n");
        let md = markdown("rapat", Some("Indonesian"), &sentences, 15.0);
        assert!(md.starts_with("# rapat\n\n_Transcript · Indonesian · 0:15_\n\n**[0:00]** Pertama."), "{md}");
        assert!(md.contains("\n\n**[0:08]** Kedua,"), "{md}");
    }

    /// A phrase's time is shared out by its words' lengths
    #[test]
    fn words_share_their_phrase_time() {
        let phrase = Segment { start: 10.0, end: 12.0, text: " Ya, betul sekali.".into(), phrase_end: true };
        let words = words(&[phrase]);
        let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, [" Ya,", " betul", " sekali."]);
        // 4 + 6 + 8 characters (each with its space) of 18
        assert!((words[0].end - (10.0 + 2.0 * 4.0 / 18.0)).abs() < 1e-9);
        assert!((words[2].start - (10.0 + 2.0 * 10.0 / 18.0)).abs() < 1e-9 && words[2].end == 12.0);
        assert_eq!(sentences(&words)[0].text, "Ya, betul sekali.");
    }

    /// Speech the model wrote without any punctuation (a voice-over over music): the
    /// model's own phrases, exactly as it returned them for a 47 s promo video, still
    /// give sentences, subtitles cut at phrase ends and paragraphs with their own
    /// timestamps, instead of one paragraph at 0:00
    #[test]
    fn unpunctuated_speech_breaks_at_phrases() {
        let phrase = |start: f64, end: f64, text: &str| Segment { start, end, text: text.into(), phrase_end: true };
        let segments = [
            phrase(0.00, 8.56, " need subtitles for your video 10 free minutes then a cue then a paywall so"),
            phrase(8.56, 18.76, " annoying enough this is flip-file drop your video pick subtitles it listens and"),
            phrase(18.76, 24.10, " writes down every word in 99 languages"),
            phrase(24.10, 34.64, " meetings lectures voice notes straight to text no upload no cue no paywall all on"),
            phrase(34.64, 42.52, " your computer on Mac Windows and Linux transcribe now in flip-file try it for"),
            phrase(42.52, 44.68, " free"),
        ];
        let words = words(&segments);
        assert!(words.last().unwrap().phrase_end && !words[0].phrase_end);
        let sentences = sentences(&words);
        assert!(sentences.len() >= 4, "{sentences:?}");
        for s in &sentences {
            assert!(s.end - s.start <= 16.0, "a sentence of {:.1} s: {}", s.end - s.start, s.text);
        }

        let md = markdown("promo", Some("English"), &sentences, 47.1);
        let stamps: Vec<&str> = md.lines().filter_map(|l| l.strip_prefix("**[")?.split(']').next()).collect();
        assert_eq!(stamps, ["0:00", "0:08", "0:18", "0:34"], "{md}");
        assert!(md.contains("**[0:18]** writes down every word"), "{md}");

        // Every cue ends where one of the model's phrases ends, within the limits
        let cues = cues(&words);
        let ends: Vec<f64> = segments.iter().map(|s| s.end).collect();
        for cue in &cues {
            assert!(cue.text.chars().count() <= CUE_CHARS && cue.end - cue.start <= CUE_SECONDS + 1e-9, "{cue:?}");
        }
        assert!(cues.iter().filter(|c| ends.iter().any(|e| (e - c.end).abs() < 1e-6)).count() >= 5, "{cues:#?}");
    }

    /// The same promo as the model writes it with its prompt: a timestamp every two or
    /// three sentences, each paragraph starting with a sentence
    #[test]
    fn punctuated_speech_gets_a_stamp_every_few_sentences() {
        let phrase = |start: f64, end: f64, text: &str| Segment { start, end, text: text.into(), phrase_end: true };
        let segments = [
            phrase(0.00, 7.64, " Need subtitles for your video? Ten free minutes, then a cue, then a paywall."),
            phrase(7.64, 13.44, " So annoying. Enough, this is FlipFile."),
            phrase(13.44, 21.60, " Drop your video, pick subtitles. It listens and writes down every word."),
            phrase(21.60, 29.44, " In 99 languages. Meetings, lectures, voice notes, straight to text."),
            phrase(30.48, 34.24, " No upload, no cue, no paywall."),
            phrase(34.24, 38.48, " All on your computer, on Mac, Windows, and Linux."),
            phrase(38.48, 46.24, " Transcribe, now in FlipFile. Try it for free."),
        ];
        let sentences = sentences(&words(&segments));
        assert_eq!(sentences.len(), 12, "{sentences:#?}");
        let md = markdown("promo", Some("English"), &sentences, 47.1);
        let paragraphs: Vec<&str> = md.lines().filter(|l| l.starts_with("**[")).collect();
        assert!(paragraphs.len() >= 4, "{md}");
        for paragraph in &paragraphs {
            let text = paragraph.split("** ").nth(1).unwrap();
            assert!(text.starts_with(char::is_uppercase) && ends_sentence(text), "{md}");
        }
    }

    /// Languages written without spaces keep none
    #[test]
    fn joins_words_as_written() {
        let words = vec![
            Segment { start: 0.0, end: 0.5, text: "今日は".into(), phrase_end: false },
            Segment { start: 0.5, end: 1.0, text: "晴れです。".into(), phrase_end: true },
        ];
        assert_eq!(sentences(&words)[0].text, "今日は晴れです。");
    }

    /// Speech spoken by macOS' own voices, written down by the bundled model: every
    /// format, both languages. (Skipped without the model, or off a Mac.)
    #[test]
    fn transcribes_speech() {
        if !whisper::available() || !cfg!(target_os = "macos") {
            eprintln!("skipped: no speech model, a processor without AVX2, or no `say`");
            return;
        }
        let dir = std::env::temp_dir().join(format!("flipfile-test-transcribe-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let speak = |voice: &str, text: &str, file: &str| -> Option<std::path::PathBuf> {
            let aiff = dir.join(format!("{file}.aiff"));
            let ok = std::process::Command::new("say").args(["-v", voice, "-o"]).arg(&aiff).arg(text).status().ok()?.success();
            // AAC in M4A, what phones record
            let m4a = dir.join(format!("{file}.m4a"));
            let ok = ok && std::process::Command::new("afconvert").args(["-f", "m4af", "-d", "aac"]).arg(&aiff).arg(&m4a).status().ok()?.success();
            ok.then_some(m4a)
        };
        let Some(english) = speak(
            "Samantha",
            "Good morning everyone. Today we will talk about automatic transcription. The recording never leaves your computer.",
            "lecture",
        ) else {
            eprintln!("skipped: this Mac has no English voice for `say`");
            return;
        };
        let steps = std::sync::Mutex::new(vec![]);
        let on_step = |done: usize, total: usize| steps.lock().unwrap().push((done, total));
        let srt = transcribe_file(&english, "tr-srt", None, &on_step).unwrap();
        assert!(srt.path.ends_with("lecture.srt"), "{}", srt.path);
        assert_eq!(srt.note.as_deref().map(|n| n.starts_with("English")), Some(true), "{:?}", srt.note);
        let text = fs::read_to_string(&srt.path).unwrap().to_lowercase();
        assert!(text.starts_with("1\n00:00:0"), "{text}");
        for word in ["morning", "automatic transcription", "computer"] {
            assert!(text.contains(word), "{word} missing from {text}");
        }
        assert_eq!(steps.lock().unwrap().last(), Some(&(100, 100)));
        for (format, ext) in [("tr-vtt", "vtt"), ("tr-txt", "txt"), ("tr-md", "md"), ("tr-docx", "docx")] {
            let out = transcribe_file(&english, format, None, &|_, _| {}).unwrap_or_else(|e| panic!("{format}: {e}"));
            assert!(out.path.ends_with(&format!(".{ext}")), "{}", out.path);
            assert!(fs::metadata(&out.path).unwrap().len() > 50, "{format} is empty");
        }

        // A voice note (Ogg Opus), decoded by the built-in Opus decoder
        let note = dir.join("voice note.opus");
        fs::write(&note, audio::tests::ogg_opus(&audio::read(&english).unwrap())).unwrap();
        let out = transcribe_file(&note, "tr-txt", None, &|_, _| {}).unwrap();
        assert!(out.path.ends_with("voice note_transcript.txt"), "{}", out.path);
        let text = fs::read_to_string(&out.path).unwrap().to_lowercase();
        assert!(text.contains("morning") && text.contains("transcription"), "{text}");

        // Indonesian, when the Mac has its voice
        if let Some(indonesian) = speak("Damayanti", "Selamat pagi semuanya. Hari ini kita akan membahas transkripsi otomatis.", "rapat") {
            let out = transcribe_file(&indonesian, "tr-txt", None, &|_, _| {}).unwrap();
            assert_eq!(out.note.as_deref().map(|n| n.starts_with("Indonesian")), Some(true), "{:?}", out.note);
            let text = fs::read_to_string(&out.path).unwrap().to_lowercase();
            assert!(text.contains("selamat pagi") && text.contains("transkripsi"), "{text}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// `FLIPFILE_TRANSCRIBE=a.m4a:b.mp4 cargo test --profile bench --lib transcribes_files -- --ignored --nocapture`:
    /// the subtitles and transcript of each file, and how long they took
    #[test]
    #[ignore]
    fn transcribes_files() {
        for path in std::env::var("FLIPFILE_TRANSCRIBE").unwrap_or_default().split(':').filter(|p| !p.is_empty()) {
            let started = std::time::Instant::now();
            let pcm = audio::read(Path::new(path)).unwrap();
            let decoded = started.elapsed();
            let transcript = whisper::transcribe(&pcm, &|_, _| {}).unwrap();
            eprintln!(
                "{path}: {:.1} s of sound, decoded in {:.2} s, transcribed in {:.1} s ({:?})\n{}",
                pcm.len() as f64 / whisper::SAMPLE_RATE as f64,
                decoded.as_secs_f64(),
                (started.elapsed() - decoded).as_secs_f64(),
                transcript.language,
                srt(&cues(&words(&transcript.segments)))
            );
            let seconds = pcm.len() as f64 / whisper::SAMPLE_RATE as f64;
            eprintln!("{}", markdown(path, transcript.language.as_deref(), &sentences(&words(&transcript.segments)), seconds));
        }
    }

    #[test]
    fn wraps_long_cues_evenly() {
        assert_eq!(wrap("short"), "short");
        assert_eq!(wrap("one two three four five six seven eight nine ten eleven"), "one two three four five six\nseven eight nine ten eleven");
        // After a comma near the middle, rather than at the exact middle
        assert_eq!(wrap("Pagi ini kita bahas bab satu dan dua, lalu kita lanjut ke latihan soal."), "Pagi ini kita bahas bab satu dan dua,\nlalu kita lanjut ke latihan soal.");
        // A comma far from the middle doesn't make lines lopsided
        assert_eq!(wrap("Hasilnya bisa disimpan sebagai subtitle, berkas teks, atau Word."), "Hasilnya bisa disimpan sebagai\nsubtitle, berkas teks, atau Word.");
    }
}
