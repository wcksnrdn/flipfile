// Resize & quality settings: a default chosen in the tray menu (persisted), which a
// single drop can override by holding Option/Alt and pointing at a preset.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Resize {
    Original,
    Scale(f32),
    MaxEdge(u32), // longest side, never upscales
}

pub struct ResizeOption {
    pub id: &'static str,
    pub menu: &'static str,
    pub short: &'static str,
    pub resize: Resize,
}

pub const RESIZE_OPTIONS: &[ResizeOption] = &[
    ResizeOption { id: "original", menu: "Original Size", short: "", resize: Resize::Original },
    ResizeOption { id: "half", menu: "50%", short: "50%", resize: Resize::Scale(0.5) },
    ResizeOption { id: "4k", menu: "Max 3840 px (4K)", short: "4K", resize: Resize::MaxEdge(3840) },
    ResizeOption { id: "1080p", menu: "Max 1920 px (Full HD)", short: "1080p", resize: Resize::MaxEdge(1920) },
    ResizeOption { id: "720p", menu: "Max 1280 px (HD)", short: "720p", resize: Resize::MaxEdge(1280) },
    ResizeOption { id: "800", menu: "Max 800 px", short: "800 px", resize: Resize::MaxEdge(800) },
];

pub struct QualityOption {
    pub id: &'static str,
    pub menu: &'static str,
    pub value: u8,
}

pub const QUALITY_OPTIONS: &[QualityOption] = &[
    QualityOption { id: "best", menu: "Best (95%)", value: 95 },
    QualityOption { id: "high", menu: "High (90%)", value: 90 },
    QualityOption { id: "balanced", menu: "Balanced (80%)", value: 80 },
    QualityOption { id: "small", menu: "Smaller File (65%)", value: 65 },
];

const DEFAULT_RESIZE: &str = "original";
const DEFAULT_QUALITY: &str = "high";
pub const COMPRESS_QUALITY: u8 = 65;

/// What one conversion actually uses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub resize: Resize,
    pub quality: u8, // JPG, AVIF and PDF; the other formats are lossless
}

impl Default for Options {
    fn default() -> Self {
        Options { resize: Resize::Original, quality: 90 }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Stored {
    resize: String,
    quality: String,
}

impl Default for Stored {
    fn default() -> Self {
        Stored { resize: DEFAULT_RESIZE.into(), quality: DEFAULT_QUALITY.into() }
    }
}

pub struct Settings {
    data: Stored,
    file: Option<PathBuf>,
}

impl Settings {
    pub fn load(file: Option<PathBuf>) -> Self {
        let data: Stored = file
            .as_ref()
            .and_then(|f| fs::read(f).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self { data, file }
    }

    fn save(&self) {
        let Some(file) = &self.file else { return };
        if let Some(dir) = file.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(&self.data) {
            let _ = fs::write(file, json);
        }
    }

    pub fn resize_id(&self) -> &str {
        &self.data.resize
    }

    pub fn quality_id(&self) -> &str {
        &self.data.quality
    }

    pub fn set_resize(&mut self, id: &str) {
        if RESIZE_OPTIONS.iter().any(|o| o.id == id) {
            self.data.resize = id.into();
            self.save();
        }
    }

    pub fn set_quality(&mut self, id: &str) {
        if QUALITY_OPTIONS.iter().any(|o| o.id == id) {
            self.data.quality = id.into();
            self.save();
        }
    }

    fn resize_option(&self) -> &'static ResizeOption {
        RESIZE_OPTIONS.iter().find(|o| o.id == self.data.resize).unwrap_or(&RESIZE_OPTIONS[0])
    }

    fn quality_value(&self) -> u8 {
        QUALITY_OPTIONS.iter().find(|o| o.id == self.data.quality).map_or(90, |o| o.value)
    }

    /// The tray defaults, optionally overridden by a preset picked with Option/Alt.
    pub fn options(&self, preset: Option<&str>) -> Options {
        let mut options = Options { resize: self.resize_option().resize, quality: self.quality_value() };
        match preset {
            Some("compress") => options.quality = COMPRESS_QUALITY.min(options.quality),
            Some(id) => {
                if let Some(o) = RESIZE_OPTIONS.iter().find(|o| o.id == id) {
                    options.resize = o.resize;
                }
            }
            None => {}
        }
        options
    }

    /// Short label of non-default settings for the UI ("1080p · 80%"), if any.
    pub fn summary(&self) -> Option<String> {
        let mut parts = vec![];
        let short = self.resize_option().short;
        if !short.is_empty() {
            parts.push(short.to_string());
        }
        if self.data.quality != DEFAULT_QUALITY {
            parts.push(format!("{}%", self.quality_value()));
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_overrides() {
        let dir = std::env::temp_dir().join(format!("flipfile-settings-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let file = dir.join("settings.json");

        let mut s = Settings::load(Some(file.clone()));
        assert_eq!(s.options(None), Options::default());
        assert_eq!(s.summary(), None);

        s.set_resize("1080p");
        s.set_quality("balanced");
        s.set_quality("nonsense"); // ignored
        let s = Settings::load(Some(file)); // persisted
        assert_eq!(s.options(None), Options { resize: Resize::MaxEdge(1920), quality: 80 });
        assert_eq!(s.summary().as_deref(), Some("1080p · 80%"));

        // Option/Alt presets override only what they are about
        assert_eq!(s.options(Some("half")), Options { resize: Resize::Scale(0.5), quality: 80 });
        assert_eq!(s.options(Some("compress")), Options { resize: Resize::MaxEdge(1920), quality: 65 });
        assert_eq!(s.options(Some("original")).resize, Resize::Original);
    }
}
