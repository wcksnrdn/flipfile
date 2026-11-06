# Changelog

All notable changes to flipfile are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **AI Summary (macOS, Apple Intelligence)**: drop a document on **AI ✦** in the lobby and a
  glass panel beside the circle explains it, written live by Apple's on-device model: document
  type, headline, the one thing to know, key points, key figures, to-dos and topics.
  - Reads PDF, Word, PowerPoint, Excel, Markdown, web pages, e-books, and scans or photos of text
    (text recognition first); legacy `.doc`/`.ppt`/`.xls` through Word, PowerPoint, Excel or LibreOffice.
  - Long documents are read part by part with live notes; very long ones are condensed to their
    most telling sentences first, so a book takes about a minute.
  - **Ask**: questions about the document (typed, or one of three it suggests) are answered from
    its most relevant passages, or with "the document doesn't say".
  - Written in the document's language; one click translates the summary into the Mac's language
    and back.
  - Copy as Markdown (`⌘C`), save as `name_summary.md` next to the original (`⌘S`, undoable from the
    tray; questions and answers included), or drop another file on the panel.
  - Runs in its own helper process (`flipfile-ai`), so the app keeps launching on every macOS
    version; the slice only appears where Apple Intelligence can run.

### Changed

- **Update check**: checking, "up to date" and errors show as a small pill under the circle, and an
  available update as a glass card; nothing dims the window or its transparent corners any more.

## [0.5.2] - 2026-11-05

### Fixed

- **PowerPoint → PDF (built-in engine)**:
  - Fonts embedded in the presentation (PowerPoint's "Embed fonts in the file", Canva and
    Google Slides exports) are now used, so slides keep their typeface where it is not installed.
  - Titles the author's app fit on one line no longer break onto a second line that overlaps the
    text below when they come out slightly wider (e.g. the original app kerned them).
  - Text boxes set not to wrap keep each line whole and stay centred or right-aligned in their box.

## [0.5.1] - 2026-11-05

### Added

- **In-App Auto-Updater (`tauri-plugin-updater`)**:
  - Automatically checks GitHub Releases for new updates with cryptographically signed releases (Minisign).
  - Background update check on startup and manual "Check for Updates…" via menu bar/tray and Ring Workshop.
  - One-click background download and automatic application relaunch.
- **Full Catalog Expansion in README**:
  - Detailed format tables covering all 75+ formats, tools, and conversion engines across Photos, Office, Data, Video, and Audio.

## [0.5.0] - 2026-11-05

### Added

- **PowerPoint conversions (`SLIDES` / `.pptx`)**:
  - **PowerPoint → PDF / images**: built-in `.pptx` reader and Typst slide renderer (4 slides per
    run) with slide → layout → master → theme inheritance, linear-RGB theme colour modifiers,
    ~70 DrawingML preset geometries and custom paths, picture fills clipped to shapes, arrowheads,
    rotated/flipped groups, SmartArt drawings, auto-fitted text, bullets/numbering and styled
    tables. Uses Microsoft PowerPoint or LibreOffice when installed, built-in engine otherwise
    (`.ppt` and `.odp` use PowerPoint or LibreOffice).
  - **PowerPoint → Word handout (`.docx`)**: printable A4 handout with each slide's image, title
    and speaker notes (2 slides per page for 16:9 decks).
  - **PowerPoint → outlines (`MD`, `TXT`, `HTML`, `RTF`, `EPUB`, `ODT`)**: slide titles, nested
    lists, tables and speaker notes in reading order.
  - **PDF, photos, documents & spreadsheets → PowerPoint (`.pptx`)**: PDF pages become slides;
    single photos become 16:9 slides and multiple photos combine into one deck (`_merged.pptx`);
    Word/Markdown/TXT/HTML/RTF/EPUB/ODT headings become slides; Excel/CSV/TSV/JSON/YAML/XML tables
    split across slides with repeating headers.
- **Ring Workshop** (`⌘,` / `Ctrl+,` or tray menu *Ring Workshop…*): a resizable panel to
  customise and reorder the rings.
- **Video & Music rooms**: audio and video conversion (`MP4`, `MOV`, `WEBM`, `MKV`, `AVI`, `MP3`,
  `M4A`, `WAV`, `FLAC`, `OGG`, `OPUS`, `AIFF`), video compression, muting, GIF clips, resizing and
  audio tools (trim silence, normalise, voice/mono, extract audio) via `ffmpeg` when installed and
  macOS's built-in `afconvert`/`avconvert` otherwise.
- **Office & Data rooms**: Markdown, plain text, HTML, RTF, ODT and EPUB conversions between each
  other and to Word/PDF/images; spreadsheets, CSV, TSV, JSON, YAML, XML, SQL and Markdown tables
  between each other; spreadsheets to Word.
- **Pro / 3D photo formats**: `TGA`, `QOI`, `EXR`, `HDR`, `PPM` and `SVG` output; `PSD`, JPEG 2000
  and camera RAW input through the system codecs.
- Stress tests (hostile, broken and extreme inputs against every format; ~3,000
  conversions, none may crash, hang or leave junk behind) and a benchmark suite
  (`npm run stress`, `npm run bench`).

### Changed

- **Faster conversions, measured on 12 MP photos** (`npm run bench`): resizing 2x faster (SIMD
  Lanczos3), target-size compression 2.5x, PDF pages to JPG 4.4x and to PNG 1.7x (pages are
  encoded on every core), HEIC to JPG 2.4x (pixels come straight from ImageIO instead of
  through a PNG), JPG to HEIC 2x, OCR and background removal skip a PNG per image.
- **Much less memory**: images are no longer copied two or three times on their way to the
  encoder (a 48 MP photo to JPG peaks at 185 MB instead of 526 MB, a batch of 24 photos at
  540 MB instead of 1.2 GB); batches take their share of a memory budget (a quarter of the
  computer's RAM), so huge photos convert only as many at once as fit; merged PDFs are
  written page by page.
- Fonts used for Word/Excel → PDF are memory-mapped instead of read: the app no longer keeps
  ~190 MB (Apple Color Emoji, a fallback font) in memory after the first conversion.
- A bug that panics while converting one odd file now fails that file only, instead of
  closing the app (release builds unwind on panic; the app is 5.5 MB larger for it).
- **Encoders adapt to the picture** (measured with SSIMULACRA2, at the same perceived
  quality): screenshots, documents and logos are told apart from photos in 2 ms.
  - AVIF is written by macOS's own encoder: 3-4x faster on photos (files ~5% larger at the
    same quality), 24-58% smaller on screenshots, documents, gradients and logos. Elsewhere
    rav1e encodes those at speed 8 (40% smaller than speed 9).
  - JPEG at the size-first qualities (Balanced, Smaller File, Compress, target sizes) stores
    colour at half resolution (4:2:0, as cameras do) with quantisation tables tuned for
    MS-SSIM: 3% smaller than before on photos, 12-28% on screenshots, documents and
    gradients. High and Best keep full colour resolution.
  - WEBP uses libwebp's *picture* tuning for screenshots, documents and logos (4% smaller).
  - Gray images (rendered text pages, black-and-white scans) are written with one channel:
    PDF pages to PNG 32% smaller, to JPG 9% smaller.
  - Compress on PNGs of 8 MP and more is 40% faster (one deflate level lower, +0.6%).
  - Compress to a target size finds the WEBP quality with libwebp's fast mode, then encodes
    once with the best one.
- **Excel → PDF typesets runs of 4 pages side by side** and puts them together into one PDF,
  with each cell's font and settings written once per document instead of per cell
  (pixel-identical): a 3,000-row workbook takes 0.6 s and 272 MB instead of 0.9 s and 677 MB,
  a 15,000-row one 3 s and 1.1 GB instead of 6 s and 2.4 GB. At most four layouts run at once
  in the whole app (more only slow each other down).
- Dropping or pasting Office documents or PDFs starts scanning the computer's fonts while you
  pick a format, so the first conversion doesn't wait for it.

### Fixed

- iPhone photos (Display P3) converted from HEIC had slightly dull colours: they are now
  converted to sRGB (pure red came out as 232, 52, 35).
- Memory grew by ~12 MB with every Word or Excel document converted and was only given back
  when the app quit (Typst's cache is now trimmed after every document).
- Converting an image wider or taller than 16,383 px to WEBP closed the app.
- Square 1:1 on a very wide panorama could allocate a canvas of many gigabytes; squares are
  now at most 16,384 px a side.
- Two files with the same name converted in one batch (`photo.png` and `photo.jpg` to WEBP)
  could both write the same output file.
- A failed conversion could leave an empty or half-written file behind.
- Word OCR now falls back to the fast recogniser when macOS' accurate one fails (it can
  break, e.g. after the disk was full).
- Images with an embedded colour profile (Mac screenshots and iPhone JPEGs are Display P3;
  also TIFF, whose profile was never read) came out with dull colours: they are converted to
  sRGB.

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

[Unreleased]: https://github.com/wcksnrdn/flipfile/compare/v0.5.2...HEAD
[0.5.2]: https://github.com/wcksnrdn/flipfile/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/wcksnrdn/flipfile/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/wcksnrdn/flipfile/compare/v0.3.0...v0.5.0
[0.3.0]: https://github.com/wcksnrdn/flipfile/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/wcksnrdn/flipfile/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/wcksnrdn/flipfile/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/wcksnrdn/flipfile/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/wcksnrdn/flipfile/releases/tag/v0.1.0
