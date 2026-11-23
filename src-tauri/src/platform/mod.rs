// Integration with the operating system: clipboard, native glass, modifier keys,
// system image codecs, background removal, AI upscaling, the menu bar / tray icon and
// the light/dark app icon.

pub mod appearance;
pub mod background;
pub mod clipboard;
pub mod faces;
pub mod glass;
pub mod keys;
// AI upscaling's engine on Windows and Linux (tested on a Mac too)
#[cfg(any(not(target_os = "macos"), test))]
pub mod onnx;
// Whether a word is a real word (Upscale Text)
pub mod spell;
pub mod system_codec;
pub mod tray;
pub mod upscale;
// Speech to text (Transcribe) with whisper.cpp
pub mod whisper;
#[cfg(any(not(target_os = "macos"), test))]
pub mod yunet;
