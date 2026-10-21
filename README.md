# flipfile

A tiny, drag-and-drop file converter for **macOS, Windows and Linux**.

flipfile lives on your desktop as a single glass circle. Drag files onto it, a ring of formats opens around it, and you drop on the one you want. The converted file appears next to the original. Nothing is uploaded: every conversion runs locally on your machine.

Built with [Tauri v2](https://tauri.app), React + TypeScript (Vite) and Rust.

---

## Contents

- [Features](#features)
- [Using flipfile](#using-flipfile)
  - [Converting by drag and drop](#converting-by-drag-and-drop)
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
| Main | JPG · JPEG · PNG · WEBP · BMP · PDF · WORD *(coming soon)* · **More** |
| More | TIFF · GIF · ICO · AVIF · **Clean** metadata · **Back** |

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

When resizing or compressing in the same format, the result shows the size change, for example `2.4 MB → 640 KB`. **Compress** lowers JPG/AVIF quality to 65% and uses maximum (still lossless) compression for PNG.

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
| BMP | ✓ | ✓ | |
| TIFF | ✓ | ✓ | |
| GIF | ✓ (first frame) | ✓ | |
| ICO | ✓ | ✓ | Fitted into a square of at most 256 × 256 |
| AVIF | ✓ *(OS codec)* | ✓ | Quality follows the Quality setting |
| HEIC / HEIF | ✓ *(OS codec)* | — | Same-format presets save iPhone photos as JPG |
| PDF | ✓ *(PDFium)* | ✓ | Images → PDF (one page per image); PDF → images at 200 DPI |
| WORD (DOCX) | — | — | Placeholder, not implemented yet |

**Clean metadata** removes EXIF, GPS, XMP, IPTC and text comments. JPG and PNG are cleaned **losslessly**; color profiles are kept. Photos with a rotation tag are re-saved upright so they don't end up sideways. Other formats are re-encoded. HEIC, AVIF and PDF can't be cleaned yet.

---

## Where results are saved

flipfile **never overwrites** a file. If a name is taken, `_2`, `_3`, … is appended.

| Action | Result |
|---|---|
| Convert `photo.png` to JPG | `photo_converted.jpg` (next to the original) |
| Several images → PDF | `firstimage_merged.pdf` |
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

The installers end up in `src-tauri/target/release/bundle/` (`.app`/`.dmg` on macOS, `.msi`/`.exe` on Windows, `.AppImage`/`.deb` on Linux). PDFium is bundled as a resource. The release app is about 17 MB on macOS, of which about 7.7 MB is PDFium.

### Checks

```bash
npm run build                      # TypeScript (strict) + Vite
cd src-tauri && cargo check        # Rust
cd src-tauri && cargo test         # Rust unit tests
```

The tests cover every output format, EXIF orientation, metadata stripping, PDF merging, PDF rendering, resize/quality, same-format presets, history and settings. Two of them need extra things:

- `renders_pdf_pages_to_images` uses the PDFium library from `npm run fetch-pdfium`.
- `decodes_heic_and_avif_via_imageio` runs only on macOS; it creates real HEIC/AVIF files with the built-in `sips` tool.

### Debug mode for drop positions

```bash
VITE_DEBUG_DROP=1 npm run tauri dev
```

This shows a red dot where flipfile thinks the cursor is, plus a readout of the raw Tauri position, `devicePixelRatio`, the window scale factor, the converted position, the angle, the current ring and the format that would be used. Every drag event is also logged to the console with the prefix `[drop-debug]`. It is off by default; adding `?debug` to the page URL works too.

---

## Project structure

```
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
    ├── capabilities/        # permissions for the UI
    ├── icons/               # app icons + tray-template.png (macOS menu bar)
    └── src/
        ├── lib.rs           # app setup: plugins, tray, shortcut, state, commands
        ├── convert.rs       # conversion pipeline, batching, progress, output names
        ├── pdf.rs           # minimal PDF writer (images → PDF)
        ├── pdf_render.rs    # PDF → images via PDFium
        ├── strip.rs         # lossless JPEG/PNG metadata removal
        ├── system_codec.rs  # HEIC/AVIF via ImageIO (macOS) / WIC (Windows)
        ├── settings.rs      # resize & quality presets, persisted
        ├── history.rs       # recent conversions + undo, persisted
        ├── tray.rs          # menu bar / tray menu, show/hide
        ├── clipboard.rs     # ⌘V / Ctrl+V: images and copied files
        ├── keys.rs          # Option/Alt state during a drag
        └── glass.rs         # native glass view behind the webview (macOS)
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
- [ ] WORD (DOCX) conversion

## License

Not decided yet.
# flipfile
