# flipfile

A tiny, drag-and-drop file converter for **macOS, Windows and Linux**.

flipfile lives on your desktop as a single glass circle. Drag files onto it, a ring of formats opens around it, and you drop on the one you want. The converted file appears next to the original. Nothing is uploaded: every conversion runs locally on your machine.

Built with [Tauri v2](https://tauri.app), React + TypeScript (Vite) and Rust.

---

## Contents

- [Features](#features)
- [Using flipfile](#using-flipfile)
  - [Converting by drag and drop](#converting-by-drag-and-drop)
  - [Word ↔ PDF](#word--pdf)
  - [More formats and "Back"](#more-formats-and-back)
  - [Resize and compress](#resize-and-compress)
  - [Pasting from the clipboard](#pasting-from-the-clipboard)
  - [After a conversion](#after-a-conversion)
  - [Menu bar / system tray](#menu-bar--system-tray)
  - [Keyboard shortcuts](#keyboard-shortcuts)
- [Supported formats](#supported-formats)
- [Where results are saved](#where-results-are-saved)
- [Platform notes](#platform-notes)
- [Privacy and stored data](#privacy-and-stored-data)
- [Security](#security)
- [Development](#development)
- [Project structure](#project-structure)
- [How it works](#how-it-works)
- [Troubleshooting](#troubleshooting)
- [Roadmap](#roadmap)

---

## Features

- **Drag-and-drop conversion** between JPG, PNG, WEBP, BMP, TIFF, GIF, ICO, AVIF and PDF.
- **Forgiving, direction-based targeting.** Move *toward* a format; you don't need to land on it exactly.
- **iPhone photos (HEIC) and AVIF input** through the operating system's own codecs.
- **Word → PDF and PDF → Word**, built in: no office suite needed (Microsoft Word or LibreOffice are used automatically when already installed, for an exact match).
- **PDF → images** (every page, 200 DPI) with PDFium, the PDF engine used by Chrome.
- **Images → one PDF.** Drop several images on PDF and get a single multi-page document.
- **Resize and compress**, either as a default in the tray menu or for one drop by holding ⌥ Option / Alt.
- **Strip metadata** (EXIF, GPS location, camera info, comments) losslessly for JPG and PNG.
- **Phone photos come out upright**: EXIF orientation is applied.
- **Paste** an image or copied files with ⌘V / Ctrl+V.
- **Never overwrites anything**: results get a new name next to the original.
- **Recent conversions and undo** (moves results to the Trash / Recycle Bin).
- **Progress** for batches and multi-page PDFs, and a spinner for anything that takes a moment.
- **Native Liquid Glass** on macOS 26+ (classic blur on older macOS, CSS glass on Windows/Linux).

---

## Using flipfile

### Converting by drag and drop

1. Drag one or more files onto the circle.
2. A ring of formats opens. Move toward a format; its slice lights up and the center shows what will happen.
3. Release to convert. Release over the center circle to cancel.

Each format owns a whole slice of the ring, so you only need to move in its direction. The center text always shows the format that will be used when you let go.

| Ring | Formats |
|---|---|
| Main | JPG · JPEG · PNG · WEBP · BMP · PDF · WORD · **More** |
| More | TIFF · GIF · ICO · AVIF · **Clean** metadata · **Back** |

### Word ↔ PDF

- **Word → PDF**: drop a `.docx` on **PDF**. Several Word files become one PDF each. Dropping a Word file on an image format gives one image per page.
- **PDF → Word**: drop a PDF on **WORD** to get an editable `.docx`.

How Word → PDF is done, best first:

| Engine | When | Result |
|---|---|---|
| Microsoft Word | Installed (Windows) | Identical to printing from Word. Word runs invisibly and is closed again afterwards. (Not used on macOS: Word for Mac is sandboxed and cannot save files for other apps.) |
| LibreOffice | Installed (any OS) | Very close to Word, run invisibly with a private profile |
| **Built-in** | Always | flipfile's own engine: reads the document (styles, fonts, lists, tables with merged cells and repeating header rows, images, text boxes, headers and footers with page numbers, tab stops, equation text) and lays it out with the [Typst](https://typst.app) typesetting engine. Page layout follows Word's own rules: every line is as tall as Word makes it for that font (its real ascent/descent), headers and footers are measured so the body starts where Word starts it, table rows include their borders and the cell padding of the table style, tables wider than the text extend into the margins, pictures fixed to the page or in front of text float, and tables of contents get live page numbers. Page breaks therefore land where Word puts them. Very specialised Word features (SmartArt, charts, tracked-change markup) are simplified. |

The user is never asked to install anything. Old binary `.doc` files need Word or LibreOffice; without them flipfile asks to save the file as `.docx`.

PDF → Word rebuilds the document from the PDF's layout, in the spirit of pdf2docx, and keeps every page on its own page with the same size and orientation:

- **Tables**: ruling lines that form a grid become real Word tables, with merged cells, borders per cell side, cell shading and row heights; tables drawn with row rules only (common in journals) and tables nested inside a cell are recognised too. Text inside cells keeps its line breaks, so rows stay as tall as in the PDF.
- **Text**: characters become lines and paragraphs with alignment, indents, exact line spacing and the spacing between paragraphs; fonts, sizes, bold, italic, underline, colour, superscript/subscript and letter-spacing are kept. Wide gaps become tabs with matching tab stops, and dotted leaders in tables of contents become real dot-leader tabs. Lines the PDF squeezed to fit are condensed slightly, so they wrap where they did.
- **Header and footer**: content repeated at the top/bottom of the pages becomes the Word header/footer; the page number becomes a live page field (also when a journal's numbering starts at e.g. 2348).
- **Layout**: blocks side by side (two-column pages, a picture beside text) are kept side by side in borderless tables, a whole column per cell. Pictures stay where they were; a picture exported in strips or tiles (a logo cut in four) is put back together at full resolution, and vector diagrams (charts, flowcharts) become pictures rendered from the page.
- **Fonts**: a font the computer lacks is replaced by the Office font of the same kind (Liberation/Tinos → Times New Roman, Arimo → Arial, Carlito → Calibri, …, or by the PDF's own serif/sans/mono description), and lines squeezed by the PDF get slightly condensed characters, so every line wraps where it did.
- **Design**: coloured bands, boxes, bars, icons, page-size artwork (covers, certificates), annotations and stamps are drawn as a picture behind the text, so the page looks like the original while the text stays editable.

Pages filled to the bottom edge get their gaps tightened by a few points, so nothing spills onto an extra page; on a test set of 30 varied PDFs (reports, journals, books, invoices, certificates, infographics, slides) every converted document has exactly as many pages as the original. Pictures are kept at most at 300 dpi of their size on the page, photo backgrounds are stored as JPEG, so the Word file is usually no bigger than the PDF.

Scanned pages without text are kept as page images by **WORD**; use **WORD OCR** to get their text.

### WORD OCR (scans and photos)

In the **More** ring, **WORD OCR** turns scanned PDFs and photos of documents (JPG, PNG, HEIC, …) into an editable Word document. The text is read by the operating system's own engine, offline, with nothing to install:

| OS | Engine | Languages |
|---|---|---|
| macOS | Vision (the engine behind Live Text) | Indonesian and English first, plus the others macOS knows |
| Windows 10/11 | Windows.Media.Ocr | the languages installed in Windows' settings |
| Linux | none built in | flipfile says so instead of converting |

The recognised text goes through the same layout reconstruction as PDF → Word (paragraphs, columns, alignment, running headers/footers, page numbers), keeps its colour, and the scan itself — with the text taken out of it — becomes the page's background, so logos, stamps, signatures, lines and photos stay where they were. A file without any readable text gives a clear message instead of an empty document.

### More formats and "Back"

Moving onto **More** swaps in the second ring. **Back** sits in the same direction as More, so the way back is where you came from. The ring only switches when you *enter* one of these slices, so it never flips back and forth while you hold still.

### Resize and compress

There are two ways, and they work together:

**1. A default for every conversion: tray menu → Resize / Quality**

| Resize | Quality *(JPG, AVIF, PDF)* |
|---|---|
| Original Size *(default)* | Best (95%) |
| 50% | High (90%) *(default)* |
| Max 3840 px (4K) | Balanced (80%) |
| Max 1920 px (Full HD) | Smaller File (65%) |
| Max 1280 px (HD) | |
| Max 800 px | |

When the defaults are changed, the circle shows a small badge such as `1080p · 80%`. Images are never enlarged.

**2. For a single drop: hold the modifier key while dragging**

| | macOS | Windows | Linux |
|---|---|---|---|
| Key | **⌥ Option** | **Alt** | *not available* (use the tray menu) |

While the key is held, the ring shows **Original · 50% · 4K · 1080p · 720p · Compress**. Then either:

- **Drop right on a preset** to keep each file's own format: `photo.jpg` → `photo_compressed.jpg` or `photo_resized.jpg`; or
- **Point at a preset, release the key, then drop on a format** to resize/compress *and* convert (for example, 1080p + WEBP).

When resizing or compressing in the same format, the result shows the size change, for example `2.4 MB → 640 KB`. **Compress** lowers JPG/AVIF quality to 65%, uses maximum (still lossless) compression for PNG and lossless Deflate compression for TIFF. BMP has no real compression, so it stays as small as BMP gets (24-bit) and flipfile says so.

> **Windows note:** holding Alt while dragging from File Explorer normally means "create shortcut". flipfile always reads the dragged files and never moves, links or changes the originals.

### Pasting from the clipboard

Click the circle, press **⌘V** (macOS) or **Ctrl+V** (Windows/Linux), then point at a format and **click**. Press **Esc** or click the center to cancel.

| Clipboard contains | Result |
|---|---|
| An image (screenshot, image copied from a browser…) | Saved in **Downloads** as `Clipboard 2026-10-20 at 14.03.12.png` (local time) |
| Files copied in Finder / File Explorer | Converted next to the originals, exactly like a drop |
| Nothing usable | "Nothing to paste" |

### After a conversion

- **Click the circle** (within a few seconds) to show the result in Finder / File Explorer.
- **Press and drag** the circle to move the window anywhere on screen.
- Errors are shown in red in the circle, for example `"notes.txt" is not a supported image`.

### Menu bar / system tray

| Item | What it does |
|---|---|
| Show / Hide flipfile | Bring the window to the front, or hide it |
| Recent Conversions | The last 10 results; click one to show it in Finder / File Explorer. *Clear History* empties the list. |
| Undo Last Conversion | Moves the results of the last conversion to the **Trash / Recycle Bin**, so they can still be restored |
| Resize, Quality | Defaults for every conversion (see above) |
| Quit flipfile | Quit the app |

On macOS the icon adapts to light and dark menu bars and clicking it opens the menu; the Dock icon brings back a hidden window. On Windows, left-click shows/hides the window and right-click opens the menu. On Linux the menu is the only entry point (tray implementations there don't report clicks).

### Keyboard shortcuts

| Shortcut | Action |
|---|---|
| **⌘⇧K** / **Ctrl+Shift+K** | Show / hide flipfile from anywhere (global) |
| **⌘V** / **Ctrl+V** | Paste an image or copied files |
| **⌥ Option** / **Alt** *(hold while dragging)* | Resize & compress presets |
| **Esc** | Cancel a paste |
| **⌘Q** / **Ctrl+Q** | Quit |

---

## Supported formats

| Format | Read | Write | Notes |
|---|:-:|:-:|---|
| JPG / JPEG | ✓ | ✓ | Transparency becomes white |
| PNG | ✓ | ✓ | Lossless |
| WEBP | ✓ | ✓ | Written lossless |
| BMP | ✓ | ✓ | Uncompressed by design; written 24-bit when there is no transparency (25% smaller) |
| TIFF | ✓ | ✓ | 24-bit when opaque; *Compress* uses lossless Deflate |
| GIF | ✓ (first frame) | ✓ | |
| ICO | ✓ | ✓ | Fitted into a square of at most 256 × 256 |
| AVIF | ✓ *(OS codec)* | ✓ | Quality follows the Quality setting |
| HEIC / HEIF | ✓ *(OS codec)* | — | Same-format presets save iPhone photos as JPG |
| PDF | ✓ *(PDFium)* | ✓ | Images → PDF (one page per image); PDF → images at 200 DPI |
| Word (DOCX) | ✓ | ✓ | Word → PDF / images; PDF → Word. See [Word ↔ PDF](#word--pdf) |
| Word 97–2003 (DOC) | ✓ *(Word/LibreOffice)* | — | Needs Word or LibreOffice installed |

**Clean metadata** removes EXIF, GPS, XMP, IPTC and text comments. JPG and PNG are cleaned **losslessly**; color profiles are kept. Photos with a rotation tag are re-saved upright so they don't end up sideways. Other formats are re-encoded. HEIC, AVIF and PDF can't be cleaned yet.

---

## Where results are saved

flipfile **never overwrites** a file. If a name is taken, `_2`, `_3`, … is appended.

| Action | Result |
|---|---|
| Convert `photo.png` to JPG | `photo_converted.jpg` (next to the original) |
| Several images → PDF | `firstimage_merged.pdf` |
| Word → PDF | `report_converted.pdf` |
| PDF → Word | `report_converted.docx` |
| Multi-page Word → PNG | folder `report_pages/` with `page-01.png`, … |
| One-page PDF → PNG | `document_converted.png` |
| Multi-page PDF → PNG | folder `document_pages/` with `page-01.png`, `page-02.png`, … |
| Clean metadata | `photo_clean.jpg` |
| Preset dropped in the same format | `photo_compressed.jpg` / `photo_resized.jpg` |
| Pasted clipboard image | `~/Downloads/Clipboard 2026-10-20 at 14.03.12.png` |

---

## Platform notes

| | macOS | Windows | Linux |
|---|---|---|---|
| HEIC / AVIF input | Built in (ImageIO) | Needs the free **HEIF Image Extensions** (and **AV1 Video Extension** for AVIF) from the Microsoft Store; flipfile tells you if they're missing | Not supported yet |
| ⌥ / Alt presets | ✓ | ✓ | Tray menu only |
| Glass effect | Native Liquid Glass (macOS 26+), blur on older versions | CSS glass | CSS glass |
| Tray click | Opens menu | Left: show/hide · Right: menu | Menu only |
| Dock / taskbar icon | ✓ | ✓ | ✓ |

---

## Privacy and stored data

flipfile works **offline**. Files never leave your computer, and the app makes no network requests.

It stores two small files in the app data folder:

| File | Contents |
|---|---|
| `history.json` | Paths of your last 50 conversions (for *Recent Conversions* and *Undo*) |
| `settings.json` | Your Resize and Quality choices |

| OS | App data folder |
|---|---|
| macOS | `~/Library/Application Support/com.satria.flipfile/` |
| Windows | `%APPDATA%\com.satria.flipfile\` |
| Linux | `~/.local/share/com.satria.flipfile/` |

Pasted clipboard images are first written to the app's cache folder; that copy is replaced on the next paste and cleared on every launch. *Clear History* in the tray menu empties `history.json`.

---

## Security

flipfile opens files from anywhere, so it is built to stay safe even if something goes wrong in one layer.

| Layer | Protection |
|---|---|
| **Content Security Policy** (`tauri.conf.json`) | The UI may only run the app's own script, style and font files. No `eval`, no inline scripts or event handlers, no remote images, frames or network requests. `default-src 'none'`. |
| **Dev server CSP** (`vite.config.ts`) | `tauri dev` gets the same policy from the Vite dev server (minus inline scripts/styles, which hot reload needs), so CSP problems show up during development. It is read from `devCsp` in `tauri.conf.json`. |
| **Frozen prototypes** | `freezePrototype` blocks prototype-pollution attacks in the webview. |
| **Least-privilege capabilities** (`capabilities/default.json`) | The UI can only listen to events, read the scale factor, move the window, reveal a file in Finder/Explorer, and call flipfile's own six commands. Opening URLs or apps, reading system paths, changing the window, registering shortcuts and anything else is refused. |
| **App commands under the ACL** (`build.rs`) | flipfile's own commands are declared in an `AppManifest`, so they are subject to capabilities like plugin commands. |
| **User-granted files only** (`src-tauri/src/state/grants.rs`) | The backend converts only files the user actually dropped or pasted, recorded from the native event before the UI sees it. The output folder is decided in Rust, never by the UI. Even injected script could not make flipfile read or write other files. |
| **Input checks** | Pasted image names are sanitised; window geometry values are range-checked. |
| **Memory-safe decoding** | Images are decoded in Rust; HEIC/AVIF by the operating system; PDFs by PDFium, pinned to a known release. |
| **Verified PDFium** (`scripts/fetch-pdfium.mjs`) | The bundled PDFium library is pinned by SHA-256, both the downloaded archive and the extracted library, and re-checked on every build. The hashes were cross-checked against the GitHub build attestations of `bblanchon/pdfium-binaries`. Any mismatch stops the build. |
| **Office apps run safely** (`src-tauri/src/office/apps.rs`) | When Word or LibreOffice is used, file paths are passed as arguments or environment variables, never pasted into script text; every run has a time limit and only that child process is stopped on timeout; LibreOffice gets a private, temporary profile. Word documents are read with size limits (no zip bombs), external links inside them are never followed, and their text is escaped before layout so document content cannot change the layout code. |
| **Offline** | No network access at runtime, no telemetry, no auto-loaded remote content. |

Download flipfile **only from this repository's GitHub Releases page**.

To report a vulnerability, please don't open a public issue; contact the maintainer privately (see `SECURITY.md` once added).

---

## Development

### Prerequisites

- [Node.js](https://nodejs.org) 20+ (developed with 24)
- [Rust](https://rustup.rs) (stable)
- The [Tauri v2 prerequisites](https://tauri.app/start/prerequisites/) for your OS (Xcode Command Line Tools on macOS, WebView2 + MSVC Build Tools on Windows, WebKitGTK and friends on Linux)
- Internet access the first time, to download PDFium

### Run

```bash
npm install
npm run tauri dev
```

`tauri dev` and `tauri build` first run `npm run fetch-pdfium`, which downloads the PDFium library for your platform into `src-tauri/pdfium/` (git-ignored). It is skipped when the right version is already there. The version is pinned in `scripts/fetch-pdfium.mjs` to match the API of the `pdfium-render` crate; update both together. To fetch for another architecture, for example when building an Intel Mac app on Apple Silicon:

```bash
PDFIUM_PLATFORM=mac-x64 npm run fetch-pdfium
```

### Build

```bash
npm run tauri build
```

The installers end up in `src-tauri/target/release/bundle/` (`.app`/`.dmg` on macOS, `.msi`/`.exe` on Windows, `.AppImage`/`.deb` on Linux). PDFium is bundled as a resource. The release app is about 42 MB on macOS (about 19 MB compressed for download): PDFium is about 7.7 MB and the Typst layout engine for Word → PDF most of the rest.

### App icon

The icon's source is `src-tauri/icons/app-icon.svg`. After editing it, export a 1024 × 1024 PNG (any SVG renderer, e.g. [resvg](https://github.com/linebender/resvg)) and regenerate every size:

```bash
resvg src-tauri/icons/app-icon.svg app-icon.png -w 1024
npm run tauri icon app-icon.png
rm -rf src-tauri/icons/android src-tauri/icons/ios   # desktop only
```

The macOS menu bar icon has its own source, `src-tauri/icons/tray-template.svg` (the same mark in plain black; macOS recolors it for light and dark menu bars):

```bash
resvg src-tauri/icons/tray-template.svg src-tauri/icons/tray-template.png -w 44 -h 44
```

### Checks

```bash
npm run build                      # TypeScript (strict) + Vite
cd src-tauri && cargo check        # Rust
cd src-tauri && cargo test         # Rust unit tests
```

The tests cover every output format, EXIF orientation, metadata stripping, PDF merging, PDF rendering, resize/quality, same-format presets, history and settings. Two of them need extra things:

- `renders_pdf_pages_to_images` uses the PDFium library from `npm run fetch-pdfium`.
- `decodes_heic_and_avif_via_imageio` runs only on macOS; it creates real HEIC/AVIF files with the built-in `sips` tool.

### Reviewing Word ↔ PDF output

Two ignored tests render conversions to PNG for a visual check:

```bash
cd src-tauri
FLIPFILE_DOCX=in.docx FLIPFILE_OUT=/tmp/out cargo test review_docx -- --ignored   # Word → PDF → PNGs
FLIPFILE_PDF=in.pdf   FLIPFILE_OUT=/tmp/out cargo test review_pdf  -- --ignored   # PDF → Word → PDF → PNGs
FLIPFILE_PDF=in.pdf   FLIPFILE_PAGE=3     cargo test dump_pdf_page -- --ignored --nocapture   # what the PDF analysis sees on a page
FLIPFILE_PDF=ours.pdf FLIPFILE_REF=ref.pdf FLIPFILE_MAP=1 cargo test compare_pages -- --ignored --nocapture   # where two PDFs' pages drift apart
```

Unit tests always use the built-in engines; they never open Word or LibreOffice.

### Debug mode for drop positions

```bash
VITE_DEBUG_DROP=1 npm run tauri dev
```

This shows a red dot where flipfile thinks the cursor is, plus a readout of the raw Tauri position, `devicePixelRatio`, the window scale factor, the converted position, the angle, the current ring and the format that would be used. Every drag event is also logged to the console with the prefix `[drop-debug]`. It is off by default; adding `?debug` to the page URL works too.

---

## Project structure

```
├── .github/workflows/
│   ├── ci.yml               # typecheck, build and Rust tests on every push / PR
│   └── release.yml          # builds installers for every OS when a v* tag is pushed
├── index.html
├── package.json
├── scripts/
│   └── fetch-pdfium.mjs     # downloads the PDFium library for the current platform
├── src/                     # UI (React + TypeScript)
│   ├── main.tsx             # entry, loads the Poppins font (bundled, offline)
│   ├── App.tsx              # drag & drop, paste, rings, presets, status, progress
│   ├── App.css              # glass look, ring and segment animations
│   ├── dropTarget.ts        # ring layout + sector hit-testing (single source of truth)
│   └── DebugOverlay.tsx     # optional drop-position diagnostics
└── src-tauri/               # native side (Rust)
    ├── tauri.conf.json      # window, bundle, identifier (com.satria.flipfile), resources
    ├── build.rs             # declares the app's commands so capabilities apply to them
    ├── capabilities/        # least-privilege permissions for the UI
    ├── icons/               # app-icon.svg + tray-template.svg (sources) → generated icons
    └── src/
        ├── lib.rs           # app setup: plugins, tray, shortcut, state, commands
        ├── convert/         # conversion pipeline behind `convert_batch`
        │   ├── mod.rs       #   batching, progress, dispatch per file
        │   ├── input.rs     #   file detection, decoding with EXIF orientation
        │   ├── encode.rs    #   resize + every image encoder
        │   ├── pdf.rs       #   PDF → images, images → merged PDF
        │   ├── word.rs      #   Word → PDF/images, OCR → Word
        │   ├── clean.rs     #   metadata removal (strip.rs: lossless JPEG/PNG)
        │   ├── output.rs    #   unique output names, never overwrites
        │   └── tests.rs
        ├── pdf/
        │   ├── writer.rs    # minimal PDF writer (images → PDF)
        │   └── render.rs    # PDF → images via PDFium
        ├── office/          # Word ↔ PDF
        │   ├── docx.rs      #   .docx reader (styles, lists, tables, images, header/footer)
        │   ├── to_typst.rs  #   document → Typst layout code (text always escaped)
        │   ├── typeset.rs   #   in-process Typst → PDF, system fonts
        │   ├── from_pdf.rs  #   PDF → page layout (tables, text, header/footer, pictures, decoration)
        │   ├── pdf_docx_writer.rs # page layout → .docx package
        │   ├── ocr.rs       #   text recognition via macOS Vision / Windows OCR
        │   └── apps.rs      #   uses Word / LibreOffice when installed, else built-in
        ├── platform/        # OS integration
        │   ├── system_codec.rs # HEIC/AVIF via ImageIO (macOS) / WIC (Windows)
        │   ├── tray.rs      #   menu bar / tray menu, show/hide
        │   ├── clipboard.rs #   ⌘V / Ctrl+V: images and copied files
        │   ├── keys.rs      #   Option/Alt state during a drag
        │   └── glass.rs     #   native glass view behind the webview (macOS)
        └── state/           # app state managed by Tauri
            ├── settings.rs  #   resize & quality presets, persisted
            ├── grants.rs    #   only files the user dropped/pasted may be converted
            └── history.rs   #   recent conversions + undo, persisted
```

---

## How it works

**One source of truth for the ring.** `src/dropTarget.ts` defines the rings (items, start angle, sector size) and the geometry (core size, ring radii). The segment shapes, the label positions and the hit-testing are all derived from it, so what you see and what gets selected cannot drift apart.

**Sector-based selection.** The cursor position is taken relative to the center of the core. Inside the core nothing is selected; outside it, the format whose angular slice contains the cursor wins. On drop the position is evaluated again, and if that lands on nothing, a valid hover from the last 150 ms is used.

**Drop coordinates per OS.** Tauri reports drag positions as `PhysicalPosition`, but the underlying webviews disagree on units (checked against wry 0.57 / tauri 2.12):

| OS | Source | Units | Conversion |
|---|---|---|---|
| macOS | `NSDraggingInfo.draggingLocation` | points (logical) | none |
| Windows | `ScreenToClient` | physical pixels | `toLogical(scaleFactor)` |
| Linux | GTK `drag-motion` | logical | none |

Dividing the macOS value by `devicePixelRatio`, as the first version did, halved every position on Retina screens and made drops miss.

**Modifier keys during a drag.** OS drag events carry no modifier keys, and the window usually isn't focused while you drag from Finder or Explorer, so the UI polls a small Rust command (`keys.rs`) every 80 ms while the ring is open. It reads `NSEvent.modifierFlags` on macOS and `GetAsyncKeyState(VK_MENU)` on Windows.

**Glass.** CSS `backdrop-filter` can only blur what is inside the webview, not the desktop behind a transparent window. On macOS, `glass.rs` places a native `NSGlassEffectView` (or `NSVisualEffectView` before macOS 26) under the webview, shaped as a circle that grows with the ring.

**Conversions** run on a worker pool sized to your CPU, off the UI thread, and report progress through a Tauri channel. PDFium renders PDFs page by page, so large documents never sit in memory all at once.

---

## Troubleshooting

| Problem | What to do |
|---|---|
| "The PDF engine is missing" | Reinstall flipfile. When developing, run `npm run fetch-pdfium`. |
| HEIC photos fail on Windows | Install **HEIF Image Extensions** from the Microsoft Store (plus **AV1 Video Extension** for AVIF). |
| ⌘⇧K / Ctrl+Shift+K does nothing | Another app has claimed the shortcut. Use the tray icon instead. |
| ⌥ / Alt presets don't appear on Linux | Not supported on Linux yet; set Resize/Quality in the tray menu. |
| A drop picks the wrong format | Run with `VITE_DEBUG_DROP=1` and check that the red dot follows the cursor. Please report the readout. |
| "Already compact" after Compress | The file was already smaller than what the preset produces (common for PNG screenshots). The new copy is kept, so you can compare. |

---

## Roadmap

- [ ] Remember the window position between sessions
- [ ] Let clicks pass through the transparent area around the circle
- [ ] Custom app icon
- [ ] GitHub Actions builds for every `v*` tag (`.dmg`, `.msi`/`.exe`, `.AppImage`/`.deb`), with macOS signing and notarization
- [ ] Auto-update
- [ ] Launch at login
- [ ] Text recognition (OCR) for scanned PDFs → Word, with the OS's own OCR
- [ ] Real tables and equation layout in PDF → Word

## License

[MIT](LICENSE) © wcksnrdn
