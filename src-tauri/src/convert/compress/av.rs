// Videos and recordings at a compress level, by bitrate (and, for video, the shorter
// side of the picture). Engines, best first:
// 1. ffmpeg, when installed: H.264 + AAC at the level's bitrates.
// 2. macOS: AVFoundation re-encodes with AVAssetReader/AVAssetWriter (H.264 + AAC, at the
//    level's bitrates; the picture's orientation is kept); afconvert for recordings.
// 3. Windows: Media Foundation (H.264 + AAC; M4A for recordings).
// Videos become MP4 and recordings M4A (an MP3 stays MP3 with ffmpeg).

use std::fs::File;
use std::path::Path;
use std::process::{Command, Stdio};

use symphonia::core::formats::FormatOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;

use super::{changed, Level, Made};
use crate::convert::input::file_name;
use crate::convert::media::{find_ffmpeg, run_cmd};
use crate::convert::output::unique_output_path;
use crate::convert::OnPage;

/// What a level makes of a video
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct VideoPlan {
    /// The picture's shorter side at most (never enlarged)
    pub short_side: u32,
    pub video_kbps: u32,
    pub audio_kbps: u32,
    pub mono: bool,
}

/// The level's plan for a video of `bytes` lasting `seconds` (if known): a bitrate that is
/// both under the level's ceiling and a share of what the video uses now, so that the
/// result is smaller whatever it started from
pub(super) fn plan_video(level: Level, bytes: u64, seconds: Option<f64>) -> VideoPlan {
    let (short_side, ceiling, share, audio_kbps, mono) = match level {
        Level::Light => (1080, 8000, 0.70, 160, false),
        Level::Balanced => (1080, 4000, 0.45, 128, false),
        Level::Strong => (720, 1800, 0.25, 96, false),
        Level::Extreme => (480, 700, 0.12, 64, true),
    };
    let video_kbps = match seconds.filter(|s| *s > 0.5) {
        Some(seconds) => {
            let total = bytes as f64 * 8.0 / seconds / 1000.0;
            let source_video = (total - audio_kbps as f64).max(total * 0.5);
            (source_video * share).min(ceiling as f64)
        }
        None => ceiling as f64,
    };
    VideoPlan { short_side, video_kbps: (video_kbps.round() as u32).max(150), audio_kbps, mono }
}

/// Bitrate of a recording at a level, and whether it becomes mono
pub(super) fn plan_audio(level: Level) -> (u32, bool) {
    match level {
        Level::Light => (160, false),
        Level::Balanced => (128, false),
        Level::Strong => (96, false),
        Level::Extreme => (48, true),
    }
}

/// How long a recording or video lasts, from its container (no decoding)
pub(super) fn duration(path: &Path) -> Option<f64> {
    let file = File::open(path).ok()?;
    let stream = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let format = symphonia::default::get_probe().probe(&hint, stream, FormatOptions::default(), MetadataOptions::default()).ok()?;
    let seconds = |tb: Option<symphonia::core::units::TimeBase>, dur: Option<symphonia::core::units::Duration>| {
        tb.zip(dur).and_then(|(tb, dur)| tb.calc_duration(dur)).map(|t| t.as_secs_f64())
    };
    let info = format.media_info();
    seconds(info.time_base, info.duration)
        .or_else(|| format.tracks().iter().filter_map(|t| seconds(t.time_base, t.duration.or(t.num_frames.map(Into::into)))).reduce(f64::max))
        .or_else(|| {
            #[cfg(target_os = "macos")]
            return mac::duration(path);
            #[allow(unreachable_code)]
            None
        })
        .filter(|s| s.is_finite() && *s > 0.0)
}

pub(super) fn compress_video(input: &Path, level: Level, out_dir: Option<&Path>, on_step: OnPage) -> Result<Made, String> {
    let name = file_name(input);
    let bytes = std::fs::metadata(input).map(|m| m.len()).unwrap_or(0);
    let seconds = duration(input);
    let plan = plan_video(level, bytes, seconds);
    let output = unique_output_path(input, out_dir, "compressed", "mp4");
    on_step(0, 100);
    let result = if let Some(ffmpeg) = find_ffmpeg() {
        ffmpeg_video(&ffmpeg, input, &output, plan, seconds, on_step, name)
    } else {
        #[cfg(target_os = "macos")]
        {
            mac::compress_video(input, &output, plan, on_step, name)
        }
        #[cfg(windows)]
        {
            win::compress_video(input, &output, plan, name)
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            Err(format!("\"{name}\": compressing videos needs ffmpeg (`sudo apt install ffmpeg`)"))
        }
    };
    if result.is_err() {
        let _ = std::fs::remove_file(&output);
    }
    result?;
    on_step(100, 100);
    Ok(Made { changed: changed(input, "mp4"), path: output })
}

pub(super) fn compress_audio(input: &Path, level: Level, out_dir: Option<&Path>) -> Result<Made, String> {
    let name = file_name(input);
    let (kbps, mono) = plan_audio(level);
    let own = input.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    let ffmpeg = find_ffmpeg();
    let ext = if own == "mp3" && ffmpeg.is_some() { "mp3" } else { "m4a" };
    let output = unique_output_path(input, out_dir, "compressed", ext);
    let result = if let Some(ffmpeg) = ffmpeg {
        let mut cmd = Command::new(ffmpeg);
        cmd.args(["-y", "-nostdin", "-v", "error", "-i"]).arg(input).arg("-vn");
        if ext == "mp3" {
            cmd.args(["-c:a", "libmp3lame"]);
        } else {
            cmd.args(["-c:a", "aac"]);
        }
        cmd.args(["-b:a", &format!("{kbps}k")]);
        if mono {
            cmd.args(["-ac", "1"]);
        }
        cmd.arg(&output);
        run_cmd(cmd, name)
    } else {
        #[cfg(target_os = "macos")]
        {
            let mut cmd = Command::new("/usr/bin/afconvert");
            cmd.args(["-f", "m4af", "-d", "aac", "-b", &(kbps * 1000).to_string()]);
            if mono {
                cmd.args(["-c", "1"]);
            }
            cmd.arg(input).arg(&output);
            run_cmd(cmd, name).map_err(|_| format!("\"{name}\": this audio format isn't readable here; install ffmpeg (`brew install ffmpeg`) for every format"))
        }
        #[cfg(windows)]
        {
            win::compress_audio(input, &output, kbps, mono, name)
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            Err(format!("\"{name}\": compressing recordings needs ffmpeg (`sudo apt install ffmpeg`)"))
        }
    };
    if result.is_err() {
        let _ = std::fs::remove_file(&output);
    }
    result?;
    Ok(Made { changed: changed(input, ext), path: output })
}

/// ffmpeg: H.264 at the plan's bitrate (with some room for hard scenes), the shorter side
/// capped, AAC sound; progress from its `-progress` output
fn ffmpeg_video(ffmpeg: &Path, input: &Path, output: &Path, plan: VideoPlan, seconds: Option<f64>, on_step: OnPage, name: &str) -> Result<(), String> {
    use std::io::{BufRead, BufReader};
    let s = plan.short_side;
    let v = plan.video_kbps;
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-y", "-nostdin", "-v", "error", "-nostats", "-progress", "pipe:1", "-i"])
        .arg(input)
        .args(["-vf", &format!("scale='if(gte(iw,ih),-2,min({s},iw))':'if(gte(iw,ih),min({s},ih),-2)'")])
        .args(["-c:v", "libx264", "-preset", "medium", "-pix_fmt", "yuv420p"])
        .args(["-b:v", &format!("{v}k"), "-maxrate", &format!("{}k", v * 3 / 2), "-bufsize", &format!("{}k", v * 2)])
        .args(["-c:a", "aac", "-b:a", &format!("{}k", plan.audio_kbps)]);
    if plan.mono {
        cmd.args(["-ac", "1"]);
    }
    cmd.args(["-movflags", "+faststart"]).arg(output);
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("\"{name}\": {e}"))?;
    if let (Some(stdout), Some(total)) = (child.stdout.take(), seconds) {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(us) = line.strip_prefix("out_time_us=").and_then(|v| v.trim().parse::<f64>().ok()) {
                on_step(((us / 1e6 / total) * 100.0).clamp(0.0, 99.0) as usize, 100);
            }
        }
    }
    let out = child.wait_with_output().map_err(|e| format!("\"{name}\": {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        Err(format!("\"{name}\": {}", stderr.lines().last().unwrap_or("compression failed").trim()))
    }
}

/// Even dimensions with the shorter side at most `short` (never enlarged)
pub(super) fn fit_short_side(w: u32, h: u32, short: u32) -> (u32, u32) {
    let k = (short as f64 / w.min(h).max(1) as f64).min(1.0);
    let even = |v: u32| (((v as f64 * k).round() as u32) & !1).max(2);
    (even(w), even(h))
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;
    use std::path::Path;

    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, Encode, Encoding, RefEncode};
    use objc2_foundation::{NSSize, NSString, NSURL};

    use super::{fit_short_side, VideoPlan};
    use crate::convert::OnPage;

    #[repr(C)]
    #[derive(Copy, Clone, Debug)]
    struct CMTime {
        value: i64,
        timescale: i32,
        flags: u32,
        epoch: i64,
    }
    unsafe impl Encode for CMTime {
        const ENCODING: Encoding = Encoding::Struct("CMTime", &[i64::ENCODING, i32::ENCODING, u32::ENCODING, i64::ENCODING]);
    }
    unsafe impl RefEncode for CMTime {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
    }
    impl CMTime {
        fn seconds(self) -> f64 {
            if self.timescale > 0 && self.flags & 1 == 1 { self.value as f64 / self.timescale as f64 } else { 0.0 }
        }
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

    /// A CMSampleBufferRef, as AVFoundation hands it out
    #[repr(transparent)]
    #[derive(Copy, Clone)]
    struct SampleBuffer(*mut c_void);
    unsafe impl Encode for SampleBuffer {
        const ENCODING: Encoding = Encoding::Pointer(&Encoding::Struct("opaqueCMSampleBuffer", &[]));
    }

    #[link(name = "AVFoundation", kind = "framework")]
    unsafe extern "C" {
        static AVMediaTypeVideo: &'static NSString;
        static AVMediaTypeAudio: &'static NSString;
        static AVFileTypeMPEG4: &'static NSString;
        static AVVideoCodecKey: &'static NSString;
        static AVVideoCodecTypeH264: &'static NSString;
        static AVVideoWidthKey: &'static NSString;
        static AVVideoHeightKey: &'static NSString;
        static AVVideoScalingModeKey: &'static NSString;
        static AVVideoScalingModeResizeAspect: &'static NSString;
        static AVVideoCompressionPropertiesKey: &'static NSString;
        static AVVideoAverageBitRateKey: &'static NSString;
        static AVVideoProfileLevelKey: &'static NSString;
        static AVVideoProfileLevelH264HighAutoLevel: &'static NSString;
        static AVFormatIDKey: &'static NSString;
        static AVNumberOfChannelsKey: &'static NSString;
        static AVSampleRateKey: &'static NSString;
        static AVEncoderBitRateKey: &'static NSString;
        static AVLinearPCMBitDepthKey: &'static NSString;
        static AVLinearPCMIsFloatKey: &'static NSString;
        static AVLinearPCMIsBigEndianKey: &'static NSString;
        static AVLinearPCMIsNonInterleaved: &'static NSString;
    }
    #[link(name = "CoreVideo", kind = "framework")]
    unsafe extern "C" {
        static kCVPixelBufferPixelFormatTypeKey: &'static NSString;
    }
    #[link(name = "CoreMedia", kind = "framework")]
    unsafe extern "C" {
        fn CMSampleBufferGetPresentationTimeStamp(buffer: *mut c_void) -> CMTime;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(object: *const c_void);
    }

    const PIXEL_FORMAT_420V: i64 = 0x34323076; // '420v'
    const AUDIO_LINEAR_PCM: i64 = 0x6C70636D; // 'lpcm'
    const AUDIO_AAC: i64 = 0x61616320; // 'aac '
    const WRITER_COMPLETED: isize = 2;
    const READER_FAILED: isize = 3;

    fn number(value: i64) -> *mut AnyObject {
        unsafe { msg_send![class!(NSNumber), numberWithLongLong: value] }
    }

    fn boolean(value: bool) -> *mut AnyObject {
        unsafe { msg_send![class!(NSNumber), numberWithBool: value] }
    }

    fn dictionary(entries: &[(&NSString, *mut AnyObject)]) -> *mut AnyObject {
        let dict: *mut AnyObject = unsafe { msg_send![class!(NSMutableDictionary), dictionary] };
        for (key, value) in entries {
            let _: () = unsafe { msg_send![dict, setObject: *value, forKey: *key] };
        }
        dict
    }

    fn error_text(error: *mut AnyObject, fallback: &str) -> String {
        if error.is_null() {
            return fallback.into();
        }
        let text: *mut NSString = unsafe { msg_send![error, localizedDescription] };
        if text.is_null() { fallback.into() } else { unsafe { (*text).to_string() } }
    }

    fn asset(path: &Path) -> Option<*mut AnyObject> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str()?));
        let asset: *mut AnyObject = unsafe { msg_send![class!(AVURLAsset), URLAssetWithURL: &*url, options: std::ptr::null::<AnyObject>()] };
        (!asset.is_null()).then_some(asset)
    }

    pub(super) fn duration(path: &Path) -> Option<f64> {
        objc2::rc::autoreleasepool(|_| {
            let asset = asset(path)?;
            let duration: CMTime = unsafe { msg_send![asset, duration] };
            Some(duration.seconds()).filter(|s| *s > 0.0)
        })
    }

    fn first_track(asset: *mut AnyObject, media: &NSString) -> Option<*mut AnyObject> {
        let tracks: *mut AnyObject = unsafe { msg_send![asset, tracksWithMediaType: media] };
        let count: usize = if tracks.is_null() { 0 } else { unsafe { msg_send![tracks, count] } };
        (count > 0).then(|| unsafe { msg_send![tracks, objectAtIndex: 0usize] })
    }

    /// Re-encodes the video track to H.264 at the plan's bitrate and size, and the first
    /// sound track to AAC; the picture keeps its rotation (portrait stays portrait)
    pub(super) fn compress_video(input: &Path, output: &Path, plan: VideoPlan, on_step: OnPage, name: &str) -> Result<(), String> {
        let unreadable = || format!("\"{name}\": macOS can't read this video's format; install ffmpeg (`brew install ffmpeg`) to compress it");
        let out_str = output.to_str().ok_or_else(|| format!("\"{name}\": invalid output path"))?;
        let _ = std::fs::remove_file(output);
        objc2::rc::autoreleasepool(|_| unsafe {
            let asset = asset(input).ok_or_else(unreadable)?;
            let video = first_track(asset, AVMediaTypeVideo).ok_or_else(unreadable)?;
            let audio = first_track(asset, AVMediaTypeAudio);
            let total: CMTime = msg_send![asset, duration];
            let total = total.seconds();
            let natural: NSSize = msg_send![video, naturalSize];
            let transform: CGAffineTransform = msg_send![video, preferredTransform];
            let (w, h) = fit_short_side(natural.width.round().max(2.0) as u32, natural.height.round().max(2.0) as u32, plan.short_side);

            let mut error: *mut AnyObject = std::ptr::null_mut();
            let reader: *mut AnyObject = msg_send![class!(AVAssetReader), assetReaderWithAsset: asset, error: &mut error];
            if reader.is_null() {
                return Err(format!("\"{name}\": {}", error_text(error, "the video could not be read")));
            }
            let video_out: *mut AnyObject = msg_send![
                class!(AVAssetReaderTrackOutput),
                assetReaderTrackOutputWithTrack: video,
                outputSettings: dictionary(&[(kCVPixelBufferPixelFormatTypeKey, number(PIXEL_FORMAT_420V))])
            ];
            let _: () = msg_send![video_out, setAlwaysCopiesSampleData: false];
            let _: () = msg_send![reader, addOutput: video_out];
            let channels = if plan.mono { 1 } else { 2 };
            let audio_out: Option<*mut AnyObject> = audio.map(|track| {
                let settings = dictionary(&[
                    (AVFormatIDKey, number(AUDIO_LINEAR_PCM)),
                    (AVNumberOfChannelsKey, number(channels)),
                    (AVSampleRateKey, number(44_100)),
                    (AVLinearPCMBitDepthKey, number(16)),
                    (AVLinearPCMIsFloatKey, boolean(false)),
                    (AVLinearPCMIsBigEndianKey, boolean(false)),
                    (AVLinearPCMIsNonInterleaved, boolean(false)),
                ]);
                let out: *mut AnyObject = msg_send![class!(AVAssetReaderTrackOutput), assetReaderTrackOutputWithTrack: track, outputSettings: settings];
                let _: () = msg_send![reader, addOutput: out];
                out
            });

            let url = NSURL::fileURLWithPath(&NSString::from_str(out_str));
            let writer: *mut AnyObject = msg_send![class!(AVAssetWriter), assetWriterWithURL: &*url, fileType: AVFileTypeMPEG4, error: &mut error];
            if writer.is_null() {
                return Err(format!("\"{name}\": {}", error_text(error, "the video could not be written")));
            }
            let compression = dictionary(&[
                (AVVideoAverageBitRateKey, number(plan.video_kbps as i64 * 1000)),
                (AVVideoProfileLevelKey, AVVideoProfileLevelH264HighAutoLevel as *const NSString as *mut AnyObject),
            ]);
            let video_settings = dictionary(&[
                (AVVideoCodecKey, AVVideoCodecTypeH264 as *const NSString as *mut AnyObject),
                (AVVideoWidthKey, number(w as i64)),
                (AVVideoHeightKey, number(h as i64)),
                (AVVideoScalingModeKey, AVVideoScalingModeResizeAspect as *const NSString as *mut AnyObject),
                (AVVideoCompressionPropertiesKey, compression),
            ]);
            let video_in: *mut AnyObject = msg_send![class!(AVAssetWriterInput), assetWriterInputWithMediaType: AVMediaTypeVideo, outputSettings: video_settings];
            let _: () = msg_send![video_in, setExpectsMediaDataInRealTime: false];
            let _: () = msg_send![video_in, setTransform: transform];
            let _: () = msg_send![writer, addInput: video_in];
            let audio_in: Option<*mut AnyObject> = audio_out.map(|_| {
                let settings = dictionary(&[
                    (AVFormatIDKey, number(AUDIO_AAC)),
                    (AVNumberOfChannelsKey, number(channels)),
                    (AVSampleRateKey, number(44_100)),
                    (AVEncoderBitRateKey, number(plan.audio_kbps as i64 * 1000)),
                ]);
                let input: *mut AnyObject = msg_send![class!(AVAssetWriterInput), assetWriterInputWithMediaType: AVMediaTypeAudio, outputSettings: settings];
                let _: () = msg_send![input, setExpectsMediaDataInRealTime: false];
                let _: () = msg_send![writer, addInput: input];
                input
            });
            let _: () = msg_send![writer, setShouldOptimizeForNetworkUse: true];

            let reading: bool = msg_send![reader, startReading];
            let writing: bool = msg_send![writer, startWriting];
            if !reading || !writing {
                let _: () = msg_send![reader, cancelReading];
                let failure: *mut AnyObject = if reading { msg_send![writer, error] } else { msg_send![reader, error] };
                return Err(format!("\"{name}\": {}", error_text(failure, "the video could not be compressed")));
            }
            let zero = CMTime { value: 0, timescale: 1, flags: 1, epoch: 0 };
            let _: () = msg_send![writer, startSessionAtSourceTime: zero];

            // Feeds both inputs as they are ready, until each runs out
            let mut lanes: Vec<(*mut AnyObject, *mut AnyObject, bool)> = vec![(video_out, video_in, false)];
            if let (Some(out), Some(input)) = (audio_out, audio_in) {
                lanes.push((out, input, false));
            }
            let mut last_percent = 0usize;
            let mut failed = false;
            while lanes.iter().any(|lane| !lane.2) && !failed {
                let mut moved = false;
                for (index, (out, input, done)) in lanes.iter_mut().enumerate() {
                    if *done {
                        continue;
                    }
                    let ready: bool = msg_send![*input, isReadyForMoreMediaData];
                    if !ready {
                        continue;
                    }
                    let buffer: SampleBuffer = msg_send![*out, copyNextSampleBuffer];
                    if buffer.0.is_null() {
                        let _: () = msg_send![*input, markAsFinished];
                        *done = true;
                        continue;
                    }
                    if index == 0 && total > 0.0 {
                        let at = CMSampleBufferGetPresentationTimeStamp(buffer.0).seconds();
                        let percent = ((at / total) * 100.0).clamp(0.0, 99.0) as usize;
                        if percent > last_percent {
                            last_percent = percent;
                            on_step(percent, 100);
                        }
                    }
                    let appended: bool = msg_send![*input, appendSampleBuffer: buffer];
                    CFRelease(buffer.0);
                    if !appended {
                        failed = true;
                        break;
                    }
                    moved = true;
                }
                let status: isize = msg_send![reader, status];
                if status == READER_FAILED {
                    failed = true;
                }
                if !moved {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
            if failed {
                let _: () = msg_send![reader, cancelReading];
                let _: () = msg_send![writer, cancelWriting];
                let failure: *mut AnyObject = msg_send![writer, error];
                return Err(format!("\"{name}\": {}", error_text(failure, "the video could not be compressed")));
            }
            let (tx, rx) = std::sync::mpsc::channel();
            let block = block2::RcBlock::new(move || {
                let _ = tx.send(());
            });
            let _: () = msg_send![writer, finishWritingWithCompletionHandler: &*block];
            let _ = rx.recv();
            let status: isize = msg_send![writer, status];
            if status != WRITER_COMPLETED {
                let failure: *mut AnyObject = msg_send![writer, error];
                return Err(format!("\"{name}\": {}", error_text(failure, "the video could not be compressed")));
            }
            Ok(())
        })
    }
}

#[cfg(windows)]
mod win {
    use std::path::Path;

    use windows::Media::MediaProperties::{AudioEncodingQuality, MediaEncodingProfile, VideoEncodingQuality};
    use windows::Media::Transcoding::{MediaTranscoder, TranscodeFailureReason};
    use windows::Storage::{CreationCollisionOption, StorageFile, StorageFolder};
    use windows::core::HSTRING;

    use super::{fit_short_side, VideoPlan};
    use crate::convert::output::save_err;

    fn transcode(input: &Path, output: &Path, name: &str, profile: impl FnOnce(&StorageFile) -> windows::core::Result<MediaEncodingProfile>) -> Result<(), String> {
        let absolute = |p: &Path| std::path::absolute(p).map_err(|e| format!("\"{name}\": {e}"));
        let source_path = HSTRING::from(absolute(input)?.as_os_str());
        let output = absolute(output)?;
        let (Some(dir), Some(file_name)) = (output.parent(), output.file_name()) else {
            return Err(save_err("invalid output path"));
        };
        let (dir, file_name) = (HSTRING::from(dir.as_os_str()), HSTRING::from(file_name));
        let run = || -> windows::core::Result<TranscodeFailureReason> {
            let source = StorageFile::GetFileFromPathAsync(&source_path)?.join()?;
            let profile = profile(&source)?;
            let folder = StorageFolder::GetFolderFromPathAsync(&dir)?.join()?;
            let target = folder.CreateFileAsync(&file_name, CreationCollisionOption::ReplaceExisting)?.join()?;
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
                "\"{name}\": Windows has no decoder for this format (iPhone videos are HEVC, which needs Microsoft's \"HEVC Video Extensions\"); or install ffmpeg (`winget install ffmpeg`)"
            )),
            Ok(_) => Err(format!("\"{name}\": Windows could not compress this file; install ffmpeg (`winget install ffmpeg`) for every format")),
            Err(e) => Err(format!("\"{name}\": {}", e.message())),
        };
        if result.is_err() {
            let _ = std::fs::remove_file(&output);
        }
        result
    }

    pub(super) fn compress_video(input: &Path, output: &Path, plan: VideoPlan, name: &str) -> Result<(), String> {
        transcode(input, output, name, |source| {
            let quality = if plan.short_side >= 1080 { VideoEncodingQuality::HD1080p } else if plan.short_side >= 720 { VideoEncodingQuality::HD720p } else { VideoEncodingQuality::Wvga };
            let profile = MediaEncodingProfile::CreateMp4(quality)?;
            let video = profile.Video()?;
            if let Ok(src) = MediaEncodingProfile::CreateFromFileAsync(source)?.join().and_then(|p| p.Video()) {
                let (w, h) = (src.Width()?, src.Height()?);
                if w > 0 && h > 0 {
                    let (w, h) = fit_short_side(w, h, plan.short_side);
                    video.SetWidth(w)?;
                    video.SetHeight(h)?;
                }
                let (from, to) = (src.FrameRate()?, video.FrameRate()?);
                if from.Numerator()? > 0 && from.Denominator()? > 0 {
                    to.SetNumerator(from.Numerator()?)?;
                    to.SetDenominator(from.Denominator()?)?;
                }
            }
            video.SetBitrate(plan.video_kbps * 1000)?;
            let audio = profile.Audio()?;
            audio.SetBitrate(plan.audio_kbps * 1000)?;
            audio.SetChannelCount(if plan.mono { 1 } else { 2 })?;
            Ok(profile)
        })
    }

    pub(super) fn compress_audio(input: &Path, output: &Path, kbps: u32, mono: bool, name: &str) -> Result<(), String> {
        transcode(input, output, name, |_| {
            let profile = MediaEncodingProfile::CreateM4a(AudioEncodingQuality::Medium)?;
            let audio = profile.Audio()?;
            audio.SetBitrate(kbps * 1000)?;
            audio.SetChannelCount(if mono { 1 } else { 2 })?;
            profile.SetVideo(None)?;
            Ok(profile)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_shrink_and_get_stronger() {
        // A 60 s Full HD phone video of 120 MB (16 Mbit/s)
        let bytes = 120 * 1024 * 1024;
        let plans: Vec<VideoPlan> = super::super::LEVELS.iter().map(|&l| plan_video(l, bytes, Some(60.0))).collect();
        for pair in plans.windows(2) {
            assert!(pair[1].video_kbps <= pair[0].video_kbps && pair[1].short_side <= pair[0].short_side, "{plans:?}");
        }
        assert_eq!(plans[0].video_kbps, 8000, "Light is capped at its ceiling");
        // A video that already has a low bitrate gets a share of it, so it still shrinks
        let small = plan_video(Level::Light, 3 * 1024 * 1024, Some(60.0));
        assert!(small.video_kbps < 400, "{small:?}");
        // Never below a floor
        assert_eq!(plan_video(Level::Extreme, 10_000, Some(600.0)).video_kbps, 150);
    }

    #[test]
    fn fits_short_side_both_ways() {
        assert_eq!(fit_short_side(1920, 1080, 720), (1280, 720));
        assert_eq!(fit_short_side(1080, 1920, 480), (480, 852));
        assert_eq!(fit_short_side(640, 360, 1080), (640, 360));
    }
}
