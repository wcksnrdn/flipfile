// Audio (Music room) and Video (Video room) conversions.
//
// Engine priority:
// 1. `ffmpeg` when installed on the computer (supports every audio & video format,
//    plus video -> animated GIF, video compression, audio stripping, and MP3/OGG/OPUS).
// 2. Built-in OS media engines on macOS (`/usr/bin/afconvert` for audio and
//    `/usr/bin/avconvert` for video/audio extraction), so WAV, M4A, AAC, ALAC, FLAC,
//    AIFF, CAF, MP4 and MOV work out of the box with zero installs.
// 3. Windows' built-in Media Foundation engine (`Windows.Media.Transcoding`) for MP4,
//    1080p, 720p and Compress, so MOV -> MP4 works there with zero installs too.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::input::file_name;
use super::output::{save_err, unique_output_path};
use super::Converted;

/// True if `format` is an audio or video target handled by this module.
pub(super) fn is_media_target(format: &str) -> bool {
    is_audio_target(format) || is_video_target(format)
}

pub(super) fn is_audio_target(format: &str) -> bool {
    matches!(
        format,
        "mp3" | "wav" | "m4a" | "flac" | "ogg" | "aiff" | "opus" | "aac" | "caf" | "wma" | "alac"
    )
}

pub(super) fn is_video_target(format: &str) -> bool {
    matches!(
        format,
        "mp4" | "mov" | "webm" | "mkv" | "avi" | "flv" | "vid-gif" | "vid-compress" | "vid-mute" | "vid-mp3" | "vid-1080p" | "vid-720p"
    )
}

fn find_ffmpeg() -> Option<PathBuf> {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut candidates: Vec<PathBuf> = vec![
        "/opt/homebrew/bin/ffmpeg".into(),
        "/usr/local/bin/ffmpeg".into(),
        "/usr/bin/ffmpeg".into(),
        "/snap/bin/ffmpeg".into(),
    ];
    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(PathBuf::from(&local).join("Microsoft/WinGet/Links/ffmpeg.exe"));
        }
        candidates.push("C:\\ffmpeg\\bin\\ffmpeg.exe".into());
    }
    if let Some(found) = candidates.into_iter().find(|p| p.is_file()) {
        return Some(found);
    }
    // Check PATH
    let ok = Command::new("ffmpeg")
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        Some(PathBuf::from("ffmpeg"))
    } else {
        None
    }
}

fn is_video_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();
    matches!(
        ext.as_str(),
        "mp4" | "mov" | "m4v" | "mkv" | "webm" | "avi" | "flv" | "wmv" | "mpeg" | "mpg" | "ts" | "3gp" | "ogv"
    )
}

pub(super) fn convert_media(input: &Path, format: &str, out_dir: Option<&Path>) -> Result<Converted, String> {
    if is_audio_target(format) {
        convert_audio(input, format, out_dir)
    } else if format == "vid-mp3" {
        if find_ffmpeg().is_some() {
            convert_audio(input, "mp3", out_dir)
        } else {
            convert_audio(input, "m4a", out_dir)
        }
    } else {
        convert_video(input, format, out_dir)
    }
}

fn convert_audio(input: &Path, format: &str, out_dir: Option<&Path>) -> Result<Converted, String> {
    let name = file_name(input);
    let (out_ext, suffix) = match format {
        "alac" => ("m4a", "alac"),
        other => (other, "converted"),
    };
    let output = unique_output_path(input, out_dir, suffix, out_ext);

    // 1. Try ffmpeg first when available (supports all formats & video-to-audio extraction)
    if let Some(ffmpeg) = find_ffmpeg() {
        let mut cmd = Command::new(ffmpeg);
        cmd.arg("-y").arg("-i").arg(input).arg("-vn");
        match format {
            "mp3" => {
                cmd.args(["-c:a", "libmp3lame", "-q:a", "2"]);
            }
            "wav" => {
                cmd.args(["-c:a", "pcm_s16le"]);
            }
            "m4a" | "aac" => {
                cmd.args(["-c:a", "aac", "-b:a", "192k"]);
            }
            "alac" => {
                cmd.args(["-c:a", "alac"]);
            }
            "flac" => {
                cmd.args(["-c:a", "flac"]);
            }
            "ogg" => {
                cmd.args(["-c:a", "libvorbis", "-q:a", "5"]);
            }
            "opus" => {
                cmd.args(["-c:a", "libopus", "-b:a", "128k"]);
            }
            "aiff" => {
                cmd.args(["-c:a", "pcm_s16be"]);
            }
            "caf" => {
                cmd.args(["-c:a", "pcm_s16le"]);
            }
            "wma" => {
                cmd.args(["-c:a", "wmav2", "-b:a", "192k"]);
            }
            _ => {}
        }
        cmd.arg(&output);
        run_cmd(cmd, &name)?;
        return Ok(Converted::file(input, &output));
    }

    // 2. macOS built-in audio converter (`/usr/bin/afconvert` + `/usr/bin/avconvert` for video inputs)
    #[cfg(target_os = "macos")]
    {
        let af_args: Option<(&str, &str)> = match format {
            "wav" => Some(("WAVE", "LEI16")),
            "aiff" => Some(("AIFF", "BEI16")),
            "caf" => Some(("caff", "LEI16")),
            "m4a" | "aac" => Some(("m4af", "aac")),
            "alac" => Some(("m4af", "alac")),
            "flac" => Some(("flac", "flac")),
            _ => None,
        };
        if let Some((file_fmt, data_fmt)) = af_args {
            // If input is a video (.mp4 / .mov), extract its audio track first with avconvert
            let mut temp_audio: Option<PathBuf> = None;
            let source_for_af = if is_video_file(input) {
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                let tmp = std::env::temp_dir().join(format!("flipfile-aud-{}-{nanos}.m4a", std::process::id()));
                let mut av = Command::new("/usr/bin/avconvert");
                av.args(["--source"])
                    .arg(input)
                    .args(["--output"])
                    .arg(&tmp)
                    .args(["--preset", "PresetAppleM4A", "--replace"]);
                run_cmd(av, &name)?;
                if format == "m4a" || format == "aac" {
                    fs::copy(&tmp, &output).map_err(save_err)?;
                    let _ = fs::remove_file(&tmp);
                    return Ok(Converted::file(input, &output));
                }
                temp_audio = Some(tmp.clone());
                tmp
            } else {
                input.to_path_buf()
            };

            let mut cmd = Command::new("/usr/bin/afconvert");
            cmd.args(["-f", file_fmt, "-d", data_fmt])
                .arg(&source_for_af)
                .arg(&output);
            let res = run_cmd(cmd, &name);
            if let Some(tmp) = temp_audio {
                let _ = fs::remove_file(tmp);
            }
            res?;
            return Ok(Converted::file(input, &output));
        }
    }

    Err(format!(
        "\"{name}\": converting to {} requires ffmpeg (install via `brew install ffmpeg` on macOS or `winget install ffmpeg` on Windows); WAV, M4A, FLAC, AIFF, CAF & ALAC work built-in on macOS",
        format.to_uppercase()
    ))
}

fn convert_video(input: &Path, format: &str, out_dir: Option<&Path>) -> Result<Converted, String> {
    let name = file_name(input);
    let src_ext = input
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .unwrap_or_default();
    let (out_ext, suffix) = match format {
        "vid-gif" => ("gif", "animated"),
        "vid-compress" => ("mp4", "compressed"),
        "vid-mute" => (muted_ext(&src_ext, cfg!(target_os = "macos") || find_ffmpeg().is_some()), "muted"),
        "vid-1080p" => ("mp4", "1080p"),
        "vid-720p" => ("mp4", "720p"),
        other => (other, "converted"),
    };
    let output = unique_output_path(input, out_dir, suffix, out_ext);

    // 1. Try ffmpeg first when available
    if let Some(ffmpeg) = find_ffmpeg() {
        let mut cmd = Command::new(ffmpeg);
        cmd.arg("-y").arg("-i").arg(input);
        match format {
            "mp4" | "mov" => {
                cmd.args(["-c:v", "libx264", "-preset", "fast", "-crf", "23", "-c:a", "aac", "-movflags", "+faststart"]);
            }
            "webm" => {
                cmd.args(["-c:v", "libvpx-vp9", "-crf", "32", "-b:v", "0", "-c:a", "libopus"]);
            }
            "mkv" => {
                cmd.args(["-c:v", "libx264", "-preset", "fast", "-crf", "23", "-c:a", "aac"]);
            }
            "avi" => {
                cmd.args(["-c:v", "mpeg4", "-q:v", "4", "-c:a", "mp3"]);
            }
            "flv" => {
                cmd.args(["-c:v", "flv", "-c:a", "mp3"]);
            }
            "vid-gif" => {
                cmd.args(["-vf", "fps=12,scale=480:-1:flags=lanczos", "-loop", "0"]);
            }
            "vid-compress" => {
                cmd.args(["-c:v", "libx264", "-preset", "medium", "-crf", "28", "-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart"]);
            }
            "vid-mute" => {
                cmd.args(["-an", "-c:v", "copy"]);
            }
            "vid-1080p" => {
                cmd.args(["-vf", "scale='min(1920,iw)':-2", "-c:v", "libx264", "-preset", "fast", "-crf", "23", "-c:a", "aac"]);
            }
            "vid-720p" => {
                cmd.args(["-vf", "scale='min(1280,iw)':-2", "-c:v", "libx264", "-preset", "fast", "-crf", "23", "-c:a", "aac"]);
            }
            _ => {}
        }
        cmd.arg(&output);
        run_cmd(cmd, &name)?;
        return Ok(Converted::file(input, &output));
    }

    // 2. macOS built-in video converter (AVFoundation passthrough for Mute, /usr/bin/avconvert for presets)
    #[cfg(target_os = "macos")]
    {
        if format == "vid-mute" {
            macos_mute(input, &output, &name)?;
            return Ok(Converted::file(input, &output));
        }

        let preset = match format {
            "mp4" | "mov" => Some("PresetHighestQuality"),
            "vid-compress" => Some("Preset960x540"),
            "vid-1080p" => Some("Preset1920x1080"),
            "vid-720p" => Some("Preset1280x720"),
            _ => None,
        };
        if let Some(p) = preset {
            let mut cmd = Command::new("/usr/bin/avconvert");
            cmd.args(["--source"])
                .arg(input)
                .args(["--output"])
                .arg(&output)
                .args(["--preset", p, "--replace"]);
            run_cmd(cmd, &name)?;
            return Ok(Converted::file(input, &output));
        }
    }

    // 3. Windows' built-in video converter (Media Foundation)
    #[cfg(windows)]
    {
        if format == "vid-mute" {
            windows_mute(input, &output, &name)?;
            return Ok(Converted::file(input, &output));
        }

        let short_side = match format {
            "mp4" => Some(None),
            "vid-compress" => Some(Some(540)),
            "vid-1080p" => Some(Some(1080)),
            "vid-720p" => Some(Some(720)),
            _ => None,
        };
        if let Some(short_side) = short_side {
            windows_transcode(input, &output, short_side, &name)?;
            return Ok(Converted::file(input, &output));
        }
    }

    Err(format!(
        "\"{name}\": converting to {} requires ffmpeg (install via `brew install ffmpeg` on macOS or `winget install ffmpeg` on Windows); MP4, MOV, Mute, 1080p, 720p & Compress work built-in on macOS, and MP4, Mute, 1080p, 720p & Compress on Windows",
        format.to_uppercase()
    ))
}

#[cfg(target_os = "macos")]
#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {}

/// The container of a muted video: a MOV stays a MOV when the engine can write one
/// (ffmpeg, macOS' AVFoundation); everything else, and Windows' own engine, gives MP4.
fn muted_ext(src_ext: &str, mov_writable: bool) -> &'static str {
    if src_ext == "mov" && mov_writable { "mov" } else { "mp4" }
}

/// Strips audio from a video using AVFoundation's passthrough preset (lossless stream copy).
/// Works out of the box on every macOS version with zero external tools.
#[cfg(target_os = "macos")]
fn macos_mute(input: &Path, output: &Path, name: &str) -> Result<(), String> {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, Encode, Encoding, RefEncode};
    use objc2_foundation::{NSString, NSURL};

    #[repr(C)]
    #[derive(Copy, Clone, Debug)]
    struct CMTime {
        value: i64,
        timescale: i32,
        flags: u32,
        epoch: i64,
    }

    unsafe impl Encode for CMTime {
        const ENCODING: Encoding = Encoding::Struct(
            "CMTime",
            &[i64::ENCODING, i32::ENCODING, u32::ENCODING, i64::ENCODING],
        );
    }
    unsafe impl RefEncode for CMTime {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
    }

    #[repr(C)]
    #[derive(Copy, Clone, Debug)]
    struct CMTimeRange {
        start: CMTime,
        duration: CMTime,
    }

    unsafe impl Encode for CMTimeRange {
        const ENCODING: Encoding = Encoding::Struct(
            "CMTimeRange",
            &[CMTime::ENCODING, CMTime::ENCODING],
        );
    }
    unsafe impl RefEncode for CMTimeRange {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
    }

    #[repr(C)]
    #[derive(Copy, Clone, Debug)]
    struct CGAffineTransform {
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        tx: f64,
        ty: f64,
    }

    unsafe impl Encode for CGAffineTransform {
        const ENCODING: Encoding = Encoding::Struct("CGAffineTransform", &[f64::ENCODING; 6]);
    }
    unsafe impl RefEncode for CGAffineTransform {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
    }

    const K_CMTIME_ZERO: CMTime = CMTime {
        value: 0,
        timescale: 1,
        flags: 1,
        epoch: 0,
    };

    let in_str = input.to_str().ok_or_else(|| format!("\"{name}\": invalid input path"))?;
    let out_str = output.to_str().ok_or_else(|| format!("\"{name}\": invalid output path"))?;

    let in_ns = NSString::from_str(in_str);
    let out_ns = NSString::from_str(out_str);

    let in_url = NSURL::fileURLWithPath(&in_ns);
    let out_url = NSURL::fileURLWithPath(&out_ns);

    let is_mov = output
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("mov"));

    objc2::rc::autoreleasepool(|_| -> Result<(), String> {
        let asset: *mut AnyObject = unsafe { msg_send![class!(AVAsset), assetWithURL: &*in_url] };
        if asset.is_null() {
            return Err(format!("\"{name}\": could not open video asset"));
        }

        let media_type = NSString::from_str("vide");
        let tracks: *mut AnyObject = unsafe { msg_send![asset, tracksWithMediaType: &*media_type] };
        let count: usize = if tracks.is_null() { 0 } else { unsafe { msg_send![tracks, count] } };
        if count == 0 {
            // AVFoundation reads MP4, MOV and M4V; not WebM, MKV, AVI or FLV
            return Err(format!(
                "\"{name}\": macOS can't read this video's format; install ffmpeg (`brew install ffmpeg`) to mute it"
            ));
        }
        let video_track: *mut AnyObject = unsafe { msg_send![tracks, objectAtIndex: 0usize] };

        let composition: *mut AnyObject = unsafe { msg_send![class!(AVMutableComposition), composition] };
        if composition.is_null() {
            return Err(format!("\"{name}\": could not create composition"));
        }

        let comp_track: *mut AnyObject = unsafe {
            msg_send![composition, addMutableTrackWithMediaType: &*media_type, preferredTrackID: 0i32]
        };
        if comp_track.is_null() {
            return Err(format!("\"{name}\": could not add composition track"));
        }

        let duration: CMTime = unsafe { msg_send![asset, duration] };
        let range = CMTimeRange {
            start: K_CMTIME_ZERO,
            duration,
        };
        let mut err: *mut AnyObject = std::ptr::null_mut();
        let inserted: bool = unsafe {
            msg_send![
                comp_track,
                insertTimeRange: range,
                ofTrack: video_track,
                atTime: K_CMTIME_ZERO,
                error: &mut err
            ]
        };
        if !inserted {
            return Err(format!("\"{name}\": could not extract video stream"));
        }
        // The track's orientation: phones store portrait videos sideways with a rotation,
        // which a composition track doesn't take over by itself
        unsafe {
            let transform: CGAffineTransform = msg_send![video_track, preferredTransform];
            let _: () = msg_send![comp_track, setPreferredTransform: transform];
        }

        let preset = NSString::from_str("AVAssetExportPresetPassthrough");
        let session: *mut AnyObject = unsafe {
            msg_send![class!(AVAssetExportSession), exportSessionWithAsset: composition, presetName: &*preset]
        };
        if session.is_null() {
            return Err(format!("\"{name}\": could not create export session"));
        }

        let file_type = if is_mov {
            NSString::from_str("com.apple.quicktime-movie")
        } else {
            NSString::from_str("public.mpeg-4")
        };

        unsafe {
            let _: () = msg_send![session, setOutputURL: &*out_url];
            let _: () = msg_send![session, setOutputFileType: &*file_type];
        }

        let _ = fs::remove_file(output);
        let (tx, rx) = std::sync::mpsc::channel();
        let block = block2::RcBlock::new(move || {
            let _ = tx.send(());
        });

        unsafe {
            let _: () = msg_send![session, exportAsynchronouslyWithCompletionHandler: &*block];
        }

        let _ = rx.recv();

        let status: isize = unsafe { msg_send![session, status] };
        // AVAssetExportSessionStatusCompleted = 3
        if status != 3 {
            let session_err: *mut AnyObject = unsafe { msg_send![session, error] };
            let desc = if !session_err.is_null() {
                let ns_desc: *mut AnyObject = unsafe { msg_send![session_err, localizedDescription] };
                if !ns_desc.is_null() {
                    let s: *const std::ffi::c_char = unsafe { msg_send![ns_desc, UTF8String] };
                    if !s.is_null() {
                        unsafe { std::ffi::CStr::from_ptr(s).to_string_lossy().to_string() }
                    } else {
                        "mute export failed".to_string()
                    }
                } else {
                    "mute export failed".to_string()
                }
            } else {
                "mute export failed".to_string()
            };
            let _ = std::fs::remove_file(output);
            return Err(format!("\"{name}\": {desc}"));
        }

        Ok(())
    })
}

/// Mutes a video (H.264 video without audio) with Windows' own Media Foundation engine.
#[cfg(windows)]
fn windows_mute(input: &Path, output: &Path, name: &str) -> Result<(), String> {
    windows_transcode_internal(input, output, None, true, name)
}

/// Converts a video to MP4 (H.264 + AAC) with Windows' own Media Foundation engine, which
/// reads MP4, MOV, M4V, WMV and AVI. `short_side` caps the shorter side of the picture
/// (1080p, 720p, Compress); `None` keeps the source size.
#[cfg(windows)]
fn windows_transcode(input: &Path, output: &Path, short_side: Option<u32>, name: &str) -> Result<(), String> {
    windows_transcode_internal(input, output, short_side, false, name)
}

#[cfg(windows)]
fn windows_transcode_internal(
    input: &Path,
    output: &Path,
    short_side: Option<u32>,
    mute: bool,
    name: &str,
) -> Result<(), String> {
    use windows::Media::MediaProperties::{MediaEncodingProfile, VideoEncodingQuality};
    use windows::Media::Transcoding::{MediaTranscoder, TranscodeFailureReason};
    use windows::Storage::{CreationCollisionOption, StorageFile, StorageFolder};
    use windows::core::HSTRING;

    // The engine wants absolute paths
    let absolute = |p: &Path| std::path::absolute(p).map_err(|e| format!("\"{name}\": {e}"));
    let source_path = HSTRING::from(absolute(input)?.as_os_str());
    let output = absolute(output)?;
    let (Some(dir), Some(file_name)) = (output.parent(), output.file_name()) else {
        return Err(save_err("invalid output path"));
    };
    let (dir, file_name) = (HSTRING::from(dir.as_os_str()), HSTRING::from(file_name));

    let run = || -> windows::core::Result<TranscodeFailureReason> {
        let source = StorageFile::GetFileFromPathAsync(&source_path)?.join()?;
        let profile = match short_side {
            None => MediaEncodingProfile::CreateMp4(VideoEncodingQuality::Auto)?,
            Some(short) => {
                let quality = match short {
                    1080 => VideoEncodingQuality::HD1080p,
                    720 => VideoEncodingQuality::HD720p,
                    _ => VideoEncodingQuality::Wvga,
                };
                let profile = MediaEncodingProfile::CreateMp4(quality)?;
                let video = profile.Video()?;
                // The preset's bitrate, with the source's aspect ratio (portrait videos
                // are not letterboxed into a landscape frame) and frame rate
                if let Ok(src) = MediaEncodingProfile::CreateFromFileAsync(&source)?.join().and_then(|p| p.Video()) {
                    let (w, h) = (src.Width()?, src.Height()?);
                    if w > 0 && h > 0 {
                        let (w, h) = fit_short_side(w, h, short);
                        video.SetWidth(w)?;
                        video.SetHeight(h)?;
                    }
                    let (from, to) = (src.FrameRate()?, video.FrameRate()?);
                    if from.Numerator()? > 0 && from.Denominator()? > 0 {
                        to.SetNumerator(from.Numerator()?)?;
                        to.SetDenominator(from.Denominator()?)?;
                    }
                }
                profile
            }
        };
        // Without an audio stream (an error here must not leave a "muted" video with sound)
        if mute {
            profile.SetAudio(None)?;
        }
        let folder = StorageFolder::GetFolderFromPathAsync(&dir)?.join()?;
        let target = folder.CreateFileAsync(&file_name, CreationCollisionOption::FailIfExists)?.join()?;
        let prepared = MediaTranscoder::new()?.PrepareFileTranscodeAsync(&source, &target, &profile)?.join()?;
        if !prepared.CanTranscode()? {
            return prepared.FailureReason();
        }
        prepared.TranscodeAsync()?.join()?;
        Ok(TranscodeFailureReason::None)
    };
    let result = match run() {
        Ok(TranscodeFailureReason::None) => Ok(()),
        Ok(TranscodeFailureReason::CodecNotFound) => Err(format!(
            "\"{name}\": Windows has no decoder for this video's format (iPhone videos are HEVC, which needs Microsoft's \"HEVC Video Extensions\"); or install ffmpeg (`winget install ffmpeg`)"
        )),
        Ok(_) => Err(format!("\"{name}\": Windows could not convert this video; install ffmpeg (`winget install ffmpeg`) for every format")),
        Err(e) => Err(format!("\"{name}\": {}", e.message())),
    };
    // No empty or half-written file is left behind (the name was free)
    if result.is_err() {
        let _ = fs::remove_file(&output);
    }
    result
}

/// The picture size with its shorter side capped at `short` (never enlarged), keeping
/// the aspect ratio and rounding to the even sizes H.264 needs.
#[cfg(any(windows, test))]
fn fit_short_side(w: u32, h: u32, short: u32) -> (u32, u32) {
    let k = (short as f64 / w.min(h) as f64).min(1.0);
    let even = |v: u32| (((v as f64 * k).round() as u32) & !1).max(2);
    (even(w), even(h))
}

#[cfg(test)]
mod tests {
    use super::{fit_short_side, muted_ext};

    #[test]
    fn muted_videos_keep_mov_where_it_can_be_written() {
        assert_eq!(muted_ext("mov", true), "mov");
        assert_eq!(muted_ext("mov", false), "mp4", "Windows' own engine writes MP4 only");
        assert_eq!(muted_ext("mp4", true), "mp4");
        assert_eq!(muted_ext("mkv", true), "mp4");
    }

    #[test]
    fn fit_short_side_keeps_orientation() {
        assert_eq!(fit_short_side(1920, 1080, 720), (1280, 720));
        assert_eq!(fit_short_side(1080, 1920, 720), (720, 1280));
        assert_eq!(fit_short_side(3840, 2160, 540), (960, 540));
        // Never enlarged
        assert_eq!(fit_short_side(640, 360, 1080), (640, 360));
        // Odd sizes become even
        assert_eq!(fit_short_side(1001, 1999, 1080), (1000, 1998));
    }
}

fn run_cmd(mut cmd: Command, name: &str) -> Result<(), String> {
    let out = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("\"{name}\": {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let msg = stderr.lines().last().unwrap_or("conversion failed").trim();
        Err(format!("\"{name}\": {msg}"))
    }
}

