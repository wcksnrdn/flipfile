// The sound of a file as 16 kHz mono samples, what Whisper hears (src/platform/whisper.rs).
//
// Engine priority:
// 1. Built-in: symphonia demuxes and decodes WAV, AIFF, CAF, FLAC, MP3, OGG (Vorbis),
//    M4A/MP4/MOV (AAC, ALAC), MKV/WebM, and the Opus in OGG/WebM (WhatsApp voice notes)
//    is decoded by opus1; both pure Rust, no temporary files.
// 2. `ffmpeg` when installed (every other codec: HE-AAC, WMA, AC-3, ...).
// 3. The system's own engines: macOS' afconvert (with avconvert for videos), Windows'
//    Media Foundation (to a temporary M4A, read by symphonia).

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::well_known::CODEC_ID_OPUS;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;

use crate::convert::input::file_name;
use crate::convert::media;
use crate::platform::whisper::SAMPLE_RATE;

/// Recordings longer than this are refused: the samples alone would take gigabytes
pub const MAX_SECONDS: usize = 5 * 60 * 60;

/// The samples of `path`, 16 kHz mono, -1 to 1
pub fn read(path: &Path) -> Result<Vec<f32>, String> {
    let name = file_name(path);
    match read_builtin(path) {
        Ok(pcm) => return Ok(pcm),
        // A recording the built-in decoders read but that holds no sound stays an error
        Err(Problem::Empty) => return Err(format!("\"{name}\" has no sound to transcribe")),
        Err(Problem::NoSoundTrack) => return Err(format!("\"{name}\" has no sound track")),
        Err(Problem::TooLong) => return Err(format!("\"{name}\" is longer than 5 hours; please split it first")),
        Err(Problem::Unsupported) => {}
    }
    if let Some(ffmpeg) = media::find_ffmpeg() {
        return read_ffmpeg(&ffmpeg, path, name);
    }
    #[cfg(target_os = "macos")]
    {
        return read_macos(path, name);
    }
    #[cfg(windows)]
    {
        return read_windows(path, name);
    }
    #[allow(unreachable_code)]
    Err(format!("\"{name}\": this audio format isn't readable here; install ffmpeg (`sudo apt install ffmpeg`) for every format"))
}

enum Problem {
    /// Not a sound file, or a codec no built-in decoder reads
    Unsupported,
    /// A video without any sound track (a screen recording, an animation)
    NoSoundTrack,
    Empty,
    TooLong,
}

/// One decoder: symphonia's, or opus1 for Opus, which gives 16 kHz mono straight away
enum Decoder {
    Symphonia(Box<dyn AudioDecoder>),
    Opus(Box<opus1::OpusDecoder>),
}

fn read_builtin(path: &Path) -> Result<Vec<f32>, Problem> {
    let unsupported = |_: &dyn std::fmt::Display| Problem::Unsupported;
    let file = File::open(path).map_err(|e| unsupported(&e))?;
    let stream = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, stream, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| unsupported(&e))?;

    // The first sound track a decoder here can read (videos carry their picture first)
    let codecs = symphonia::default::get_codecs();
    if !format.tracks().iter().any(|track| matches!(track.codec_params, Some(CodecParameters::Audio(_)))) {
        return Err(Problem::NoSoundTrack);
    }
    let (track_id, params, delay) = format
        .tracks()
        .iter()
        .find_map(|track| match &track.codec_params {
            Some(CodecParameters::Audio(params)) if params.codec == CODEC_ID_OPUS || codecs.get_audio_decoder(params.codec).is_some() => {
                Some((track.id, params.clone(), track.delay.unwrap_or(0)))
            }
            _ => None,
        })
        .ok_or(Problem::Unsupported)?;
    let mut decoder = if params.codec == CODEC_ID_OPUS {
        let decoder = opus1::OpusDecoder::new(opus1::SampleRate::Hz16000, opus1::Channels::Mono).map_err(|e| unsupported(&e))?;
        Decoder::Opus(Box::new(decoder))
    } else {
        Decoder::Symphonia(codecs.make_audio_decoder(&params, &AudioDecoderOptions::default()).map_err(|e| unsupported(&e))?)
    };

    let mut out = Vec::new();
    let mut resampler: Option<Resampler> = None;
    // Opus packets have a few milliseconds of codec delay in front (in their own 48 kHz)
    let mut skip = if params.codec == CODEC_ID_OPUS { delay as usize * SAMPLE_RATE / params.sample_rate.unwrap_or(48_000).max(1) as usize } else { 0 };
    let mut opus_frame = vec![0f32; SAMPLE_RATE * 120 / 1000];
    let mut mono = Vec::new();
    let mut interleaved = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            // A cut-off file ends where it ends
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => {
                if out.is_empty() {
                    return Err(unsupported(&e));
                }
                break;
            }
        };
        if packet.track_id != track_id {
            continue;
        }
        match &mut decoder {
            Decoder::Opus(opus) => {
                let Ok(n) = opus.decode(&packet.data, opus_frame.len(), &mut opus_frame) else { continue };
                let samples = &opus_frame[..n];
                let from = skip.min(samples.len());
                skip -= from;
                out.extend_from_slice(&samples[from..]);
            }
            Decoder::Symphonia(dec) => {
                let buffer: GenericAudioBufferRef<'_> = match dec.decode(&packet) {
                    Ok(buffer) => buffer,
                    // One bad packet (a glitch in a stream) is skipped
                    Err(SymphoniaError::DecodeError(_)) => continue,
                    Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                    Err(e) => {
                        if out.is_empty() {
                            return Err(unsupported(&e));
                        }
                        break;
                    }
                };
                if buffer.frames() == 0 {
                    continue;
                }
                let spec = buffer.spec();
                let (rate, channels) = (spec.rate() as usize, spec.channels().count().max(1));
                interleaved.clear();
                buffer.copy_to_vec_interleaved::<f32>(&mut interleaved);
                downmix(&interleaved, channels, &mut mono);
                let resampler = resampler.get_or_insert_with(|| Resampler::new(rate, SAMPLE_RATE));
                resampler.push(&mono, &mut out);
            }
        }
        if out.len() > MAX_SECONDS * SAMPLE_RATE {
            return Err(Problem::TooLong);
        }
    }
    if let Some(resampler) = resampler {
        resampler.finish(&mut out);
    }
    if out.len() < SAMPLE_RATE / 10 {
        return Err(Problem::Empty);
    }
    Ok(out)
}

/// Every channel averaged into one
fn downmix(interleaved: &[f32], channels: usize, mono: &mut Vec<f32>) {
    mono.clear();
    if channels == 1 {
        mono.extend_from_slice(interleaved);
        return;
    }
    let scale = 1.0 / channels as f32;
    mono.extend(interleaved.chunks_exact(channels).map(|frame| frame.iter().sum::<f32>() * scale));
}

/// Streams samples from one rate to another: a windowed-sinc low-pass filter read at the
/// output's positions (a polyphase table of `PHASES` kernels), so what is above the
/// output's Nyquist frequency doesn't fold back as noise. Mono.
pub struct Resampler {
    /// Input samples per output sample
    ratio: f64,
    /// Taps on each side of the centre
    half: usize,
    /// `PHASES` kernels of `2 * half + 1` taps each, one per fraction of a sample
    table: Vec<f32>,
    /// Input not yet left behind, starting with `half` zeros of history
    pending: Vec<f32>,
    /// Where the next output sample falls in `pending`
    pos: f64,
}

const PHASES: usize = 256;

impl Resampler {
    pub fn new(from: usize, to: usize) -> Self {
        let ratio = from as f64 / to as f64;
        // The filter has to cut at the lower of the two Nyquist frequencies; a slower
        // cut-off (downsampling) needs a longer kernel for the same steepness
        let cutoff = 0.5 / ratio.max(1.0) * 0.92;
        let half = (12.0 * ratio.max(1.0)).ceil() as usize;
        let width = 2 * half + 1;
        let mut table = vec![0f32; PHASES * width];
        for phase in 0..PHASES {
            let frac = phase as f64 / PHASES as f64;
            let kernel = &mut table[phase * width..(phase + 1) * width];
            let mut sum = 0.0;
            for (k, tap) in kernel.iter_mut().enumerate() {
                let t = k as f64 - half as f64 - frac; // the input sample's distance from the output position
                let x = std::f64::consts::PI * t;
                // sin(2πct) / (πt), whose limit at t = 0 is 2c
                let sinc = if x.abs() < 1e-9 { 2.0 * cutoff } else { (2.0 * cutoff * x).sin() / x };
                // Blackman window over the kernel's span
                let w = {
                    let u = (t / (half as f64 + 1.0)).clamp(-1.0, 1.0);
                    let a = std::f64::consts::PI * (u + 1.0);
                    0.42 - 0.5 * a.cos() + 0.08 * (2.0 * a).cos()
                };
                let v = sinc * w;
                *tap = v as f32;
                sum += v;
            }
            // Unity gain for a steady signal
            for tap in kernel.iter_mut() {
                *tap = (*tap as f64 / sum) as f32;
            }
        }
        Resampler { ratio, half, table, pending: vec![0.0; half], pos: half as f64 }
    }

    /// Takes more input and appends what can be computed to `out`
    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.pending.extend_from_slice(input);
        self.drain(out);
    }

    /// The last output samples once the input has ended
    pub fn finish(mut self, out: &mut Vec<f32>) {
        self.pending.extend(std::iter::repeat_n(0.0, self.half + 1));
        self.drain(out);
    }

    fn drain(&mut self, out: &mut Vec<f32>) {
        let width = 2 * self.half + 1;
        loop {
            let index = self.pos.floor();
            let centre = index as usize;
            if centre + self.half + 1 > self.pending.len() {
                break;
            }
            let mut phase = ((self.pos - index) * PHASES as f64).round() as usize;
            let mut centre = centre;
            if phase == PHASES {
                phase = 0;
                centre += 1;
                if centre + self.half + 1 > self.pending.len() {
                    break;
                }
            }
            let kernel = &self.table[phase * width..(phase + 1) * width];
            let window = &self.pending[centre - self.half..centre + self.half + 1];
            out.push(window.iter().zip(kernel).map(|(s, k)| s * k).sum());
            self.pos += self.ratio;
        }
        // History older than the kernel needs is let go of
        let keep_from = (self.pos.floor() as usize).saturating_sub(self.half);
        if keep_from > 0 {
            self.pending.drain(..keep_from);
            self.pos -= keep_from as f64;
        }
    }
}

/// ffmpeg decodes anything, straight into 16 kHz mono floats on its standard output
fn read_ffmpeg(ffmpeg: &Path, input: &Path, name: &str) -> Result<Vec<f32>, String> {
    let out = Command::new(ffmpeg)
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(input)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-f", "f32le", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("\"{name}\": {e}"))?;
    if !out.status.success() || out.stdout.len() < 4 * SAMPLE_RATE / 10 {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let reason = stderr.lines().last().unwrap_or("no sound to transcribe").trim();
        return Err(format!("\"{name}\": {reason}"));
    }
    if out.stdout.len() > 4 * MAX_SECONDS * SAMPLE_RATE {
        return Err(format!("\"{name}\" is longer than 5 hours; please split it first"));
    }
    Ok(out.stdout.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

/// A temporary file name of flipfile's own
fn temp_path(ext: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    std::env::temp_dir().join(format!("flipfile-speech-{}-{nanos}.{ext}", std::process::id()))
}

/// macOS: afconvert writes 16 kHz mono floats of whatever CoreAudio opens (MP3, AAC,
/// WMA-free formats, ...); a video's sound is first taken out by avconvert
#[cfg(target_os = "macos")]
fn read_macos(input: &Path, name: &str) -> Result<Vec<f32>, String> {
    let wav = temp_path("wav");
    let convert = |source: &Path| -> Result<(), String> {
        let mut cmd = Command::new("/usr/bin/afconvert");
        cmd.args(["-f", "WAVE", "-d", "LEF32@16000", "-c", "1"]).arg(source).arg(&wav);
        media::run_cmd(cmd, name)
    };
    let mut result = convert(input);
    let mut temp_audio = None;
    if result.is_err() && media::is_video_file(input) {
        let m4a = temp_path("m4a");
        let mut av = Command::new("/usr/bin/avconvert");
        av.args(["--source"]).arg(input).args(["--output"]).arg(&m4a).args(["--preset", "PresetAppleM4A", "--replace"]);
        result = media::run_cmd(av, name).and_then(|_| convert(&m4a));
        temp_audio = Some(m4a);
    }
    let pcm = result
        .map_err(|_| format!("\"{name}\": this audio format isn't readable here; install ffmpeg (`brew install ffmpeg`) for every format"))
        .and_then(|_| read_builtin(&wav).map_err(|_| format!("\"{name}\" has no sound to transcribe")));
    let _ = std::fs::remove_file(&wav);
    if let Some(m4a) = temp_audio {
        let _ = std::fs::remove_file(m4a);
    }
    pcm
}

/// Windows: Media Foundation writes the sound to a temporary M4A (AAC), which the
/// built-in decoder reads
#[cfg(windows)]
fn read_windows(input: &Path, name: &str) -> Result<Vec<f32>, String> {
    let m4a = temp_path("m4a");
    let result = media::windows_extract_audio(input, &m4a, name)
        .and_then(|_| read_builtin(&m4a).map_err(|_| format!("\"{name}\" has no sound to transcribe")));
    let _ = std::fs::remove_file(&m4a);
    result
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A tone resampled keeps its pitch and length
    #[test]
    fn resampler_keeps_pitch_and_length() {
        for from in [44_100usize, 48_000, 22_050, 8_000, 16_000] {
            let seconds = 2.0;
            let n = (from as f64 * seconds) as usize;
            let tone: Vec<f32> = (0..n).map(|i| (2.0 * std::f64::consts::PI * 440.0 * i as f64 / from as f64).sin() as f32).collect();
            let mut out = Vec::new();
            let mut resampler = Resampler::new(from, SAMPLE_RATE);
            // In uneven pieces, as decoders hand them out
            for chunk in tone.chunks(1234) {
                resampler.push(chunk, &mut out);
            }
            resampler.finish(&mut out);
            let expected = (SAMPLE_RATE as f64 * seconds) as usize;
            assert!((out.len() as i64 - expected as i64).abs() <= 2, "{from} Hz: {} samples, expected {expected}", out.len());
            // Zero crossings count the frequency (the middle second, away from the edges)
            let middle = &out[SAMPLE_RATE / 2..SAMPLE_RATE * 3 / 2];
            let crossings = middle.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
            assert!((crossings as i64 - 880).abs() <= 4, "{from} Hz: {crossings} zero crossings, expected 880");
            // Full amplitude (no attenuation at 440 Hz)
            let peak = middle.iter().fold(0f32, |m, v| m.max(v.abs()));
            assert!((peak - 1.0).abs() < 0.03, "{from} Hz: peak {peak}");
        }
    }

    /// What lies above the new Nyquist frequency is filtered out, not folded back
    #[test]
    fn resampler_removes_aliases() {
        let from = 48_000usize;
        let n = from * 2;
        // 10 kHz: inaudible to a 16 kHz signal (Nyquist 8 kHz); it would alias to 6 kHz
        let tone: Vec<f32> = (0..n).map(|i| (2.0 * std::f64::consts::PI * 10_000.0 * i as f64 / from as f64).sin() as f32).collect();
        let mut out = Vec::new();
        let mut resampler = Resampler::new(from, SAMPLE_RATE);
        resampler.push(&tone, &mut out);
        resampler.finish(&mut out);
        let peak = out[SAMPLE_RATE / 2..SAMPLE_RATE * 3 / 2].iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.02, "alias peak {peak}");
    }

    /// `FLIPFILE_AUDIO=a.mp4:b.ogg cargo test --lib decodes_files -- --ignored --nocapture`:
    /// how long each file's decoded sound is, for checking a decoder against real files
    #[test]
    #[ignore]
    fn decodes_files() {
        for path in std::env::var("FLIPFILE_AUDIO").unwrap_or_default().split(':').filter(|p| !p.is_empty()) {
            let builtin = read_builtin(Path::new(path)).is_ok();
            match read(Path::new(path)) {
                Ok(pcm) => eprintln!("{path}: {:.2} s ({})", pcm.len() as f64 / SAMPLE_RATE as f64, if builtin { "built-in" } else { "system" }),
                Err(e) => eprintln!("{path}: {e}"),
            }
        }
    }

    #[test]
    fn downmix_averages_channels() {
        let mut mono = Vec::new();
        downmix(&[1.0, 0.0, 0.5, 0.5, -1.0, 1.0], 2, &mut mono);
        assert_eq!(mono, vec![0.5, 0.5, 0.0]);
    }

    /// A WAV written by hound-free hand (16-bit stereo 44.1 kHz) decodes to 16 kHz mono
    #[test]
    fn reads_wav() {
        let rate = 44_100u32;
        let frames = rate as usize * 3 / 2;
        let mut data = Vec::with_capacity(frames * 4);
        for i in 0..frames {
            let v = ((2.0 * std::f64::consts::PI * 300.0 * i as f64 / rate as f64).sin() * 12_000.0) as i16;
            data.extend_from_slice(&v.to_le_bytes());
            data.extend_from_slice(&v.to_le_bytes());
        }
        let path = std::env::temp_dir().join(format!("flipfile-test-wav-{}.wav", std::process::id()));
        std::fs::write(&path, wav_bytes(rate, 2, &data)).unwrap();
        let pcm = read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let expected = SAMPLE_RATE * 3 / 2;
        assert!((pcm.len() as i64 - expected as i64).abs() <= 2, "{} samples", pcm.len());
        let peak = pcm.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!((peak - 12_000.0 / 32_768.0).abs() < 0.02, "peak {peak}");
    }

    /// An Ogg Opus file, as WhatsApp and Telegram record voice notes: `pcm` (16 kHz
    /// mono) encoded by opus1 at 24 kbit/s, 20 ms a packet
    pub(crate) fn ogg_opus(pcm: &[f32]) -> Vec<u8> {
        use ogg::writing::{PacketWriteEndInfo, PacketWriter};
        let mut encoder = opus1::OpusEncoder::new(opus1::SampleRate::Hz16000, opus1::Channels::Mono, opus1::Application::Voip).unwrap();
        encoder.set_bitrate(opus1::Bitrate::BitsPerSecond(std::num::NonZeroU32::new(24_000).unwrap()));
        let pre_skip = (encoder.lookahead() * 3) as u16; // in 48 kHz samples
        let mut head = b"OpusHead".to_vec();
        head.push(1); // version
        head.push(1); // channels
        head.extend_from_slice(&pre_skip.to_le_bytes());
        head.extend_from_slice(&16_000u32.to_le_bytes());
        head.extend_from_slice(&0i16.to_le_bytes()); // gain
        head.push(0); // mapping family
        let mut tags = b"OpusTags".to_vec();
        tags.extend_from_slice(&7u32.to_le_bytes());
        tags.extend_from_slice(b"flipfile");
        tags.truncate(8 + 4 + 7);
        tags.extend_from_slice(&0u32.to_le_bytes());

        let mut out = Vec::new();
        {
            let mut writer = PacketWriter::new(&mut out);
            writer.write_packet(head, 7, PacketWriteEndInfo::EndPage, 0).unwrap();
            writer.write_packet(tags, 7, PacketWriteEndInfo::EndPage, 0).unwrap();
            const FRAME: usize = 320;
            let frames: Vec<&[f32]> = pcm.chunks(FRAME).collect();
            let mut granule = pre_skip as u64;
            for (i, frame) in frames.iter().enumerate() {
                let mut padded = frame.to_vec();
                padded.resize(FRAME, 0.0);
                let mut packet = vec![0u8; 1500];
                let n = encoder.encode(&padded, FRAME, &mut packet).unwrap();
                packet.truncate(n);
                granule += (frame.len() * 3) as u64;
                let end = if i + 1 == frames.len() { PacketWriteEndInfo::EndStream } else { PacketWriteEndInfo::NormalPacket };
                writer.write_packet(packet, 7, end, granule).unwrap();
            }
        }
        out
    }

    /// A voice note decodes to as long as it was recorded, its pre-skip left out
    #[test]
    fn reads_ogg_opus() {
        let n = SAMPLE_RATE * 2;
        let tone: Vec<f32> = (0..n).map(|i| (2.0 * std::f64::consts::PI * 300.0 * i as f64 / SAMPLE_RATE as f64).sin() as f32 * 0.5).collect();
        let path = std::env::temp_dir().join(format!("flipfile-test-opus-{}.opus", std::process::id()));
        std::fs::write(&path, ogg_opus(&tone)).unwrap();
        let pcm = read_builtin(&path).ok().expect("built-in Opus decoding");
        let _ = std::fs::remove_file(&path);
        assert!((pcm.len() as i64 - n as i64).abs() <= 320, "{} samples, expected {n}", pcm.len());
        let peak = pcm[SAMPLE_RATE / 2..SAMPLE_RATE].iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.1, "peak {peak}");
    }

    /// A 16-bit PCM WAV file around `data`
    pub(crate) fn wav_bytes(rate: u32, channels: u16, data: &[u8]) -> Vec<u8> {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        wav.extend_from_slice(&(channels * 2).to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(data);
        wav
    }
}
