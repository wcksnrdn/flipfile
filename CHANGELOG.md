# Changelog

All notable changes to flipfile are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.3.0] - 2026-11-03

### Added

- **Category Rooms**: dropping or pasting files now opens a category lobby (**Photo · Office · Video · Music**), with dedicated sub-rings per room and aligned **Back ‹** navigation.
- **Photo → Tools**:
  - **Remove BG** (macOS 14+): offline subject background removal via macOS Vision (`VNGenerateForegroundInstanceMaskRequest`), saved as a transparent PNG (`_nobg.png`).
  - **Compress to target size** (**1 MB · 500 KB · 200 KB · 100 KB**): binary search over encoder quality and progressive resolution scaling to fit strict byte budgets.
  - **Lossless compress**: `oxipng` optimisation for PNG, lossless metadata stripping for JPEG, lossless WebP, and Deflate for TIFF.
  - **Transform**: rotate 90° CW, 90° CCW, 180°, flip horizontal/vertical, and grayscale (B&W).
  - **Square 1:1**: pads non-square photos to a 1:1 square without cropping.
  - **GIF anim**: combines multiple images into a looping animated GIF (`_animated.gif`).
- **HEIC output** (macOS): native HEIC writing via ImageIO (`CGImageDestination`).
- **ICNS & multi-size ICO**: `.ico` files now pack `16, 32, 48, 64, 128, 256` px slices; new **ICNS** output packs `16`–`1024` px slices (`icp4`–`ic10`) for macOS app icons.

### Changed

- **WEBP output is now lossy** (via `libwebp`) and follows the Quality setting, producing compact files instead of oversized lossless WebP.

## [0.2.0] - 2026-11-02

### Added

- **Excel → PDF** (`.xlsx`, `.xlsm`, `.csv`; `.xls`/`.ods` with Excel or LibreOffice).
  Microsoft Excel (Windows) or LibreOffice are used when installed; otherwise a built-in
  engine prints every visible sheet like Excel: page setup, fit to page, print area,
  repeated title rows, page breaks, header/footer, number formats (accounting, Rupiah,
  dates, percentages) in the region's own separators, fonts, fills, borders, merged
  cells, wrapped text and pictures.

### Changed

- README: animated demos of every feature, a download guide, and the details in
  collapsible sections.

## [0.1.2] - 2026-10-30

### Changed

- New logo: two nested "f" strokes, in a clean monochrome palette.
- The app icon follows the system appearance: light by default, dark in dark mode
  (on macOS while the app runs; "Auto" appearance keeps the light icon).
- New menu bar icon (macOS) with the same mark.

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

[Unreleased]: https://github.com/wcksnrdn/flipfile/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/wcksnrdn/flipfile/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/wcksnrdn/flipfile/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/wcksnrdn/flipfile/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/wcksnrdn/flipfile/releases/tag/v0.1.0
