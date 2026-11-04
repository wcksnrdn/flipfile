// Audio (Music room) and Video (Video room) conversions.
//
// Engine priority:
// 1. `ffmpeg` when installed on the computer (supports every audio & video format,
//    plus video -> animated GIF, video compression, audio stripping, and MP3/OGG/OPUS).
// 2. Built-in OS media engines on macOS (`/usr/bin/afconvert` for audio and
//    `/usr/bin/avconvert` for video/audio extraction), so WAV, M4A, AAC, ALAC, FLAC,
//    AIFF, CAF, MP4 and MOV work out of the box with zero installs.

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
    let (out_ext, suffix) = match format {
        "vid-gif" => ("gif", "animated"),
        "vid-compress" => ("mp4", "compressed"),
        "vid-mute" => ("mp4", "muted"),
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

    // 2. macOS built-in video converter (`/usr/bin/avconvert`)
    #[cfg(target_os = "macos")]
    {
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

    Err(format!(
        "\"{name}\": converting to {} requires ffmpeg (install via `brew install ffmpeg` on macOS or `winget install ffmpeg` on Windows); MP4, MOV, 1080p, 720p & Compress work built-in on macOS",
        format.to_uppercase()
    ))
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
