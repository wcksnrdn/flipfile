<div align="center">

<img src="src-tauri/icons/128x128@2x.png" width="112" alt="flipfile icon">

# flipfile

**Drop a file. Point to a format. Done.**

A tiny file converter that lives on your desktop.<br>
Photos, PDFs, Word, Excel and scans, converted right on your computer.<br>
No uploads. No accounts. No waiting.

[![Download](https://img.shields.io/github/v/release/wcksnrdn/flipfile?label=download&color=1d1d1f&style=for-the-badge)](https://github.com/wcksnrdn/flipfile/releases/latest)
[![Build](https://img.shields.io/github/actions/workflow/status/wcksnrdn/flipfile/ci.yml?branch=main&label=build&style=for-the-badge)](https://github.com/wcksnrdn/flipfile/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-6e6e73?style=for-the-badge)](LICENSE)

![macOS](https://img.shields.io/badge/macOS-Apple%20Silicon%20%26%20Intel-f5f5f7?logo=apple&logoColor=1d1d1f)
![Windows](https://img.shields.io/badge/Windows-10%20%26%2011-f5f5f7?logo=windows&logoColor=0078d4)
![Linux](https://img.shields.io/badge/Linux-deb%20%C2%B7%20rpm%20%C2%B7%20AppImage-f5f5f7?logo=linux&logoColor=1d1d1f)

<br>

<img src="docs/media/hero.webp" width="100%" alt="flipfile on a Mac desktop: a photo is dragged onto the glass circle, the ring of formats opens, JPG is picked and the converted file appears next to the original">

<br>

[**Download**](https://github.com/wcksnrdn/flipfile/releases/latest) &nbsp;·&nbsp; [See it in action](#see-it-in-action) &nbsp;·&nbsp; [How to use](#how-to-use) &nbsp;·&nbsp; [Formats](#supported-formats)

</div>

<br>

## Why flipfile

| **Private by design** | **Instant** | **Does the boring stuff** |
|---|---|---|
| Every conversion runs on your computer. Files never leave it, and flipfile keeps working with the Wi-Fi off. | No upload, no queue, no "your download will start in 30 seconds". Drop, point, done. | iPhone photos, PDF ⇄ Word, Excel → PDF, scans → editable text, resize and compress, metadata cleaning. |

<br>

## See it in action

<table>
<tr>
<td width="50%" valign="top">

**Drag, point, let go**<br>
<sub>Move toward a format, its slice lights up, release. The result lands next to the original.</sub>

<img src="docs/media/drag.webp" width="100%" alt="Dragging a HEIC photo onto flipfile and releasing it on JPG">

</td>
<td width="50%" valign="top">

**Works offline**<br>
<sub>Turn the Wi-Fi off: everything still converts, because nothing is ever uploaded.</sub>

<img src="docs/media/offline.webp" width="100%" alt="Switching Wi-Fi off from the menu bar">

</td>
</tr>
<tr>
<td width="50%" valign="top">

**A whole batch at once**<br>
<sub>Select ten photos, drop them together, watch them finish one by one.</sub>

<img src="docs/media/batch.webp" width="100%" alt="Ten photos converted to WEBP at once, each getting a check mark">

</td>
<td width="50%" valign="top">

**PDF → Word you can edit**<br>
<sub>Tables, columns, headers and pictures are rebuilt, not pasted as images.</sub>

<img src="docs/media/pdf.webp" width="100%" alt="A PDF converted to Word and opened, with its table and two columns highlighted">

</td>
</tr>
<tr>
<td width="50%" valign="top">

**Scans and photos → text**<br>
<sub>WORD OCR reads receipts and scanned pages with your computer's own text recognition.</sub>

<img src="docs/media/ocr.webp" width="100%" alt="A photo of a receipt turned into an editable Word document">

</td>
<td width="50%" valign="top">

**Resize and compress**<br>
<sub>Hold ⌥ Option (Alt on Windows) while dragging for 4K, 1080p, 720p, 50% or a smaller file.</sub>

<img src="docs/media/compress.webp" width="100%" alt="Holding Option shows the size ring; Compress makes the photo 85% smaller">

</td>
</tr>
</table>

### Excel → PDF that looks like Excel printed it

<img align="right" src="docs/media/excel-to-pdf.webp" width="46%" alt="A profit and loss statement printed from Excel by flipfile, with Rupiah accounting formats and a double-underlined total">

Drop a workbook on **PDF** and every visible sheet prints the way Excel prints it: page setup and fit to page, repeated title rows, headers and footers with page numbers, merged cells, borders and fills, logos, and **number formats exactly as Excel shows them**, including accounting and Rupiah formats with negatives in brackets.

Separators follow your region (1.234.567,89 in Indonesia, 1,234,567.89 in the US), and dates follow your language.

Excel or LibreOffice are used when installed; otherwise flipfile's own engine does it. Nothing to install either way.

<br clear="right">

<br>

## Download

Get the latest version from **[Releases](https://github.com/wcksnrdn/flipfile/releases/latest)**:

| Your computer | File |
|---|---|
| Mac with Apple Silicon (M1 to M4) | `flipfile_x.y.z_aarch64.dmg` |
| Mac with Intel | `flipfile_x.y.z_x64.dmg` |
| Windows 10 / 11 | `flipfile_x.y.z_x64-setup.exe` |
| Linux | Debian/Ubuntu `.deb` &nbsp;·&nbsp; Fedora `.rpm` &nbsp;·&nbsp; any distro `.AppImage` |

<details>
<summary><b>First launch: "can't be verified" or "Windows protected your PC"</b></summary>

flipfile isn't notarized by Apple or code-signed for Windows yet, so the first launch needs one extra click:

- **macOS**: open flipfile, click **Done**, then go to **System Settings → Privacy & Security** and click **Open Anyway**. Only once.
- **Windows**: click **More info → Run anyway**.
- **Linux**: `sudo apt install ./flipfile_*.deb`, `sudo dnf install ./flipfile-*.rpm`, or `chmod +x` the AppImage and run it.

</details>

<br>

## How to use

1. **Drag** files onto the glass circle. A ring of formats opens around it.
2. **Point** toward a format. You don't have to hit it exactly: its whole slice lights up, and the centre tells you what will happen.
3. **Let go.** The converted file appears next to the original. Click the circle to show it.

| Ring | Formats |
|---|---|
| Main | JPG · JPEG · PNG · WEBP · BMP · PDF · WORD · **More** |
| More | TIFF · GIF · ICO · AVIF · **Clean** metadata · **WORD OCR** · **Back** |

Also handy: **⌘V / Ctrl+V** pastes an image or copied files, **⌘⇧K / Ctrl+Shift+K** shows or hides flipfile from anywhere, and the menu bar / tray icon keeps your recent conversions with **Undo**.

<details>
<summary><b>Word ↔ PDF</b></summary>

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

</details>

<details>
<summary><b>Excel → PDF</b></summary>

Drop an Excel workbook (`.xlsx`, `.xlsm`) or a `.csv` on **PDF**; on an image format it gives one image per page. Like Word → PDF, Microsoft Excel (Windows) or LibreOffice are used when installed; otherwise flipfile's built-in engine prints the workbook itself, so nothing has to be installed. Old `.xls` and `.ods` files need Excel or LibreOffice.

The built-in engine prints what Excel would print:

- **Every visible sheet**, each with its own page setup: paper size (A4, Letter, Legal, F4, …), orientation, margins, scale or *fit to N pages wide/tall*, centring, print area, rows repeated at the top of each page, manual page breaks, page order, and header/footer with page numbers (`&P`, `&N`), sheet and file name. Hidden sheets, rows and columns don't print.
- **Numbers as Excel shows them**: Excel's number formats are reproduced, including accounting and currency formats (`Rp`, `$`, the symbol at the cell's left edge and negatives in brackets), `[Red]` negatives, percentages, thousands scaling, scientific notation, fractions, and dates and times. Thousands and decimal separators follow the computer's region (1.234.567,89 in Indonesia, 1,234,567.89 in the US), and day/month names follow its language. Formulas print their last calculated result, as stored by Excel.
- **The look of the sheet**: column widths and row heights in Excel's own units, fonts, fills, borders (thin, thick, double, dashed, …), merged cells, alignment and indents, wrapped text (rows without a fixed height grow to fit it), pictures such as logos, and printed gridlines when the sheet asks for them. Text spills into empty neighbouring cells, numbers that don't fit show `####`, as in Excel.
- **CSV** files print as a table with gridlines and a bold header row; `,` `;` tab and `|` separators are detected, and codes with leading zeros stay as written.

Charts, shapes and conditional formatting are not drawn by the built-in engine yet.

</details>

<details>
<summary><b>WORD OCR (scans and photos)</b></summary>

In the **More** ring, **WORD OCR** turns scanned PDFs and photos of documents (JPG, PNG, HEIC, …) into an editable Word document. The text is read by the operating system's own engine, offline, with nothing to install:

| OS | Engine | Languages |
|---|---|---|
| macOS | Vision (the engine behind Live Text) | Indonesian and English first, plus the others macOS knows |
| Windows 10/11 | Windows.Media.Ocr | the languages installed in Windows' settings |
| Linux | none built in | flipfile says so instead of converting |

The recognised text goes through the same layout reconstruction as PDF → Word (paragraphs, columns, alignment, running headers/footers, page numbers), keeps its colour, and the scan itself, with the text taken out of it, becomes the page's background, so logos, stamps, signatures, lines and photos stay where they were. A file without any readable text gives a clear message instead of an empty document.

</details>

<details>
<summary><b>More formats and "Back"</b></summary>

Moving onto **More** swaps in the second ring. **Back** sits in the same direction as More, so the way back is where you came from. The ring only switches when you *enter* one of these slices, so it never flips back and forth while you hold still.

</details>

<details>
<summary><b>Resize and compress</b></summary>

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

</details>

<details>
<summary><b>Pasting from the clipboard</b></summary>

Click the circle, press **⌘V** (macOS) or **Ctrl+V** (Windows/Linux), then point at a format and **click**. Press **Esc** or click the center to cancel.

| Clipboard contains | Result |
|---|---|
| An image (screenshot, image copied from a browser…) | Saved in **Downloads** as `Clipboard 2026-10-20 at 14.03.12.png` (local time) |
| Files copied in Finder / File Explorer | Converted next to the originals, exactly like a drop |
| Nothing usable | "Nothing to paste" |

</details>

<details>
<summary><b>After a conversion</b></summary>

- **Click the circle** (within a few seconds) to show the result in Finder / File Explorer.
- **Press and drag** the circle to move the window anywhere on screen.
- Errors are shown in red in the circle, for example `"notes.txt" is not a supported image`.

</details>

<details>
<summary><b>Menu bar / system tray</b></summary>

| Item | What it does |
|---|---|
| Show / Hide flipfile | Bring the window to the front, or hide it |
| Recent Conversions | The last 10 results; click one to show it in Finder / File Explorer. *Clear History* empties the list. |
| Undo Last Conversion | Moves the results of the last conversion to the **Trash / Recycle Bin**, so they can still be restored |
| Resize, Quality | Defaults for every conversion (see above) |
| Quit flipfile | Quit the app |

On macOS the icon adapts to light and dark menu bars and clicking it opens the menu; the Dock icon brings back a hidden window. On Windows, left-click shows/hides the window and right-click opens the menu. On Linux the menu is the only entry point (tray implementations there don't report clicks).

</details>

<details>
<summary><b>Keyboard shortcuts</b></summary>

| Shortcut | Action |
|---|---|
| **⌘⇧K** / **Ctrl+Shift+K** | Show / hide flipfile from anywhere (global) |
| **⌘V** / **Ctrl+V** | Paste an image or copied files |
| **⌥ Option** / **Alt** *(hold while dragging)* | Resize & compress presets |
| **Esc** | Cancel a paste |
| **⌘Q** / **Ctrl+Q** | Quit |

</details>

<br>

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
| Word (DOCX) | ✓ | ✓ | Word → PDF / images; PDF → Word. See [How to use](#how-to-use) |
| Word 97–2003 (DOC) | ✓ *(Word/LibreOffice)* | — | Needs Word or LibreOffice installed |
| Excel (XLSX, XLSM), CSV | ✓ | — | Excel → PDF / images. See [Excel → PDF](#excel--pdf-that-looks-like-excel-printed-it) |
| Excel 97–2003 (XLS), ODS | ✓ *(Excel/LibreOffice)* | — | Needs Excel or LibreOffice installed |

**Clean metadata** removes EXIF, GPS, XMP, IPTC and text comments. JPG and PNG are cleaned **losslessly**; color profiles are kept. Photos with a rotation tag are re-saved upright so they don't end up sideways. Other formats are re-encoded. HEIC, AVIF and PDF can't be cleaned yet.

<details>
<summary><b>Where results are saved</b></summary>

flipfile **never overwrites** a file. If a name is taken, `_2`, `_3`, … is appended.

| Action | Result |
|---|---|
| Convert `photo.png` to JPG | `photo_converted.jpg` (next to the original) |
| Several images → PDF | `firstimage_merged.pdf` |
| Word → PDF | `report_converted.pdf` |
| Excel → PDF | `budget_converted.pdf` (all visible sheets in one PDF) |
| PDF → Word | `report_converted.docx` |
| Multi-page Word → PNG | folder `report_pages/` with `page-01.png`, … |
| One-page PDF → PNG | `document_converted.png` |
| Multi-page PDF → PNG | folder `document_pages/` with `page-01.png`, `page-02.png`, … |
| Clean metadata | `photo_clean.jpg` |
| Preset dropped in the same format | `photo_compressed.jpg` / `photo_resized.jpg` |
| Pasted clipboard image | `~/Downloads/Clipboard 2026-10-20 at 14.03.12.png` |

</details>

<details>
<summary><b>Platform notes</b></summary>

| | macOS | Windows | Linux |
|---|---|---|---|
| HEIC / AVIF input | Built in (ImageIO) | Needs the free **HEIF Image Extensions** (and **AV1 Video Extension** for AVIF) from the Microsoft Store; flipfile tells you if they're missing | Not supported yet |
| ⌥ / Alt presets | ✓ | ✓ | Tray menu only |
| Glass effect | Native Liquid Glass (macOS 26+), blur on older versions | CSS glass | CSS glass |
| Tray click | Opens menu | Left: show/hide · Right: menu | Menu only |
| Dock / taskbar icon | ✓ | ✓ | ✓ |

</details>

<br>

## Privacy

flipfile works **offline**: files never leave your computer, the app makes no network requests and has no telemetry. It only remembers your recent conversions and your resize and quality choices.

<details>
<summary><b>Stored data, in detail</b></summary>

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

</details>

<details>
<summary><b>Security</b></summary>

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
| **Office apps run safely** (`src-tauri/src/office/apps.rs`) | When Word, Excel or LibreOffice is used, file paths are passed as arguments or environment variables, never pasted into script text; every run has a time limit and only that child process is stopped on timeout; LibreOffice gets a private, temporary profile. Word documents are read with size limits (no zip bombs), external links inside them are never followed, and their text is escaped before layout so document content cannot change the layout code. |
| **Offline** | No network access at runtime, no telemetry, no auto-loaded remote content. |

Download flipfile **only from this repository's GitHub Releases page**.

To report a vulnerability, please don't open a public issue; contact the maintainer privately (see `SECURITY.md` once added).

</details>

<br>

## For developers

<details>
<summary><b>Development: run, build, test</b></summary>

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

### Reviewing Word ↔ PDF and Excel → PDF output

Two ignored tests render conversions to PNG for a visual check:

```bash
cd src-tauri
FLIPFILE_DOCX=in.docx FLIPFILE_OUT=/tmp/out cargo test review_docx -- --ignored   # Word → PDF → PNGs
FLIPFILE_PDF=in.pdf   FLIPFILE_OUT=/tmp/out cargo test review_pdf  -- --ignored   # PDF → Word → PDF → PNGs
FLIPFILE_XLSX=in.xlsx FLIPFILE_OUT=/tmp/out cargo test review_xlsx -- --ignored   # Excel/CSV → PDF → PNGs (FLIPFILE_SEP=, for 1.234,56)
FLIPFILE_PDF=in.pdf   FLIPFILE_PAGE=3     cargo test dump_pdf_page -- --ignored --nocapture   # what the PDF analysis sees on a page
FLIPFILE_PDF=ours.pdf FLIPFILE_REF=ref.pdf FLIPFILE_MAP=1 cargo test compare_pages -- --ignored --nocapture   # where two PDFs' pages drift apart
```

Unit tests always use the built-in engines; they never open Word, Excel or LibreOffice.

### Debug mode for drop positions

```bash
VITE_DEBUG_DROP=1 npm run tauri dev
```

This shows a red dot where flipfile thinks the cursor is, plus a readout of the raw Tauri position, `devicePixelRatio`, the window scale factor, the converted position, the angle, the current ring and the format that would be used. Every drag event is also logged to the console with the prefix `[drop-debug]`. It is off by default; adding `?debug` to the page URL works too.

</details>

<details>
<summary><b>Project structure</b></summary>

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
        ├── office/          # Word ↔ PDF, Excel → PDF
        │   ├── docx.rs      #   .docx reader (styles, lists, tables, images, header/footer)
        │   ├── to_typst.rs  #   document → Typst layout code (text always escaped)
        │   ├── typeset.rs   #   in-process Typst → PDF, system fonts
        │   ├── from_pdf.rs  #   PDF → page layout (tables, text, header/footer, pictures, decoration)
        │   ├── pdf_docx_writer.rs # page layout → .docx package
        │   ├── ocr.rs       #   text recognition via macOS Vision / Windows OCR
        │   ├── xlsx.rs      #   .xlsx / .csv reader (cells, styles, page setup, pictures)
        │   ├── numfmt.rs    #   Excel number formats, in the region's separators
        │   ├── sheet_pdf.rs #   prints sheets into pages, like Excel
        │   └── apps.rs      #   uses Word / Excel / LibreOffice when installed, else built-in
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

</details>

<details>
<summary><b>How it works</b></summary>

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

</details>

<details>
<summary><b>Troubleshooting</b></summary>

| Problem | What to do |
|---|---|
| "The PDF engine is missing" | Reinstall flipfile. When developing, run `npm run fetch-pdfium`. |
| HEIC photos fail on Windows | Install **HEIF Image Extensions** from the Microsoft Store (plus **AV1 Video Extension** for AVIF). |
| ⌘⇧K / Ctrl+Shift+K does nothing | Another app has claimed the shortcut. Use the tray icon instead. |
| ⌥ / Alt presets don't appear on Linux | Not supported on Linux yet; set Resize/Quality in the tray menu. |
| A drop picks the wrong format | Run with `VITE_DEBUG_DROP=1` and check that the red dot follows the cursor. Please report the readout. |
| "Already compact" after Compress | The file was already smaller than what the preset produces (common for PNG screenshots). The new copy is kept, so you can compare. |

</details>

<br>

## Roadmap

- [x] Custom app icon, light and dark
- [x] Builds for every OS on each release (`.dmg`, `.exe`/`.msi`, `.deb`/`.rpm`/`.AppImage`)
- [x] Text recognition (OCR) for scans and photos → Word
- [x] Real tables in PDF → Word
- [x] Excel → PDF
- [ ] macOS notarization and Windows code signing
- [ ] Remember the window position between sessions
- [ ] Let clicks pass through the transparent area around the circle
- [ ] Auto-update
- [ ] Launch at login
- [ ] Charts in Excel → PDF (built-in engine)

<br>

<div align="center">

<img src="src-tauri/icons/128x128.png" width="64" alt="">

**flipfile** &nbsp;·&nbsp; Drop. Flip. Done.

[**Try it for free**](https://github.com/wcksnrdn/flipfile/releases/latest)

<sub>[MIT](LICENSE) © wcksnrdn</sub>

</div>
