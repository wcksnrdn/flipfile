//! Audio formats through the system's converter (macOS)

use super::*;

#[cfg(target_os = "macos")]
#[test]
fn converts_audio_formats_via_system_afconvert() {
    let dir = temp_dir("audio-conv");
    let wav = dir.join("tone.wav");

    // Write a valid 0.1s 8kHz mono 16-bit PCM WAV file (800 samples = 1600 bytes)
    let samples: Vec<i16> = (0..800).map(|i| ((i * 40) % 4000 - 2000) as i16).collect();
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // 1 channel
    bytes.extend_from_slice(&8000u32.to_le_bytes()); // 8000 Hz
    bytes.extend_from_slice(&16000u32.to_le_bytes()); // byte rate
    bytes.extend_from_slice(&2u16.to_le_bytes()); // block align
    bytes.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    fs::write(&wav, &bytes).unwrap();

    for (fmt, ext) in [("m4a", ".m4a"), ("flac", ".flac"), ("aiff", ".aiff"), ("caf", ".caf")] {
        let out = run(wav.to_str().unwrap(), fmt, None, &Options::default()).unwrap();
        assert!(out.ends_with(ext), "expected {ext}, got {out}");
        assert!(fs::metadata(&out).unwrap().len() > 32);
    }
}
