# Changelog

All notable changes to flipfile are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.1] - 2026-10-27

### Fixed

- macOS: the downloaded app was reported as "damaged and can't be opened". The whole
  bundle is now ad-hoc signed after its resources are added.

### Added

- macOS Intel (x64) build alongside Apple Silicon.

## [0.1.0] - 2026-10-23

First public release.

### Added

- Drag-and-drop conversion between JPG, PNG, WEBP, BMP, TIFF, GIF, ICO, AVIF and PDF.
- HEIC and AVIF input through the operating system's own codecs.
- PDF → images (PDFium), several images → one merged PDF.
- Word → PDF via Word / LibreOffice when installed, otherwise a built-in Typst engine.
- PDF → Word with tables, headers/footers and layout rebuilt from the page geometry.
- OCR of scans and photos into Word (macOS Vision, Windows OCR).
- Resize and compress presets (Option/Alt ring), lossless metadata cleaning.
- Clipboard paste, menu bar / tray with recent conversions and undo, global shortcut.
- Native glass look on macOS.

[Unreleased]: https://github.com/wcksnrdn/flipfile/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/wcksnrdn/flipfile/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/wcksnrdn/flipfile/releases/tag/v0.1.0
