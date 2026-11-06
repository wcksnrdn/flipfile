# Contributing to flipfile

Thank you for your interest in contributing to **flipfile**! Whether you are fixing a bug, adding a new file format, improving conversion accuracy, or refining the radial ring UI, your help makes offline file conversion better for everyone.

By participating in this project, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

---

## Core Design Principles

Before submitting a change, please keep flipfile's core philosophy in mind:

1. **100% Offline & Private by Design** — Every conversion runs locally on the user's machine. Never add network requests, cloud APIs, telemetry, or external font/script CDNs.
2. **Drop. Point. Done.** — The UI stays tiny, instant, and distraction-free. Complex options belong in smart defaults, the radial category rooms, or the **Ring Workshop**.
3. **Never Overwrite Originals** — Converted files are always written next to the source file with a clear suffix (`_converted`, `_clean`, `_nobg`, `_2`, etc.), and failed conversions must never leave empty or half-written files behind.
4. **Defense-in-Depth Security** — The WebView UI is treated as untrusted. File paths are authorized exclusively by native OS drag/paste events in `src-tauri/src/state/grants.rs`, and every Tauri command is gated by `src-tauri/build.rs` and `src-tauri/capabilities/default.json`.

---

## Getting Started

### Prerequisites

* **[Node.js](https://nodejs.org/) 20+** (developed with Node 24)
* **[Rust](https://rustup.rs/) (stable toolchain)**
* **[Tauri v2 OS Prerequisites](https://tauri.app/start/prerequisites/)**:
  * **macOS:** Xcode Command Line Tools (`xcode-select --install`); with the macOS 26 SDK or later the build also compiles `flipfile-ai` for AI summaries (with an older SDK the app builds without them)
  * **Windows:** Microsoft Visual Studio C++ Build Tools + WebView2
  * **Linux:** `libwebkit2gtk-4.1-dev`, `build-essential`, `curl`, `wget`, `file`, `libxdo-dev`, `libssl-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`

### Local Development

```bash
# 1. Install frontend dependencies
npm install

# 2. Start the app in development mode (automatically downloads & verifies PDFium on first run)
npm run tauri dev
```

> **Note:** On the first run, `scripts/fetch-pdfium.mjs` downloads the pinned PDFium binary for your platform into `src-tauri/pdfium/` and verifies its SHA-256 checksum.

---

## Architecture Overview

```text
├── src/                         # Frontend (React 19 + TypeScript + Vite)
│   ├── App.tsx                  # Drag-and-drop, clipboard paste, radial rings, Core state
│   ├── App.css                  # Glassmorphism, radial segments, Workshop styles
│   ├── dropTarget.ts            # Single source of truth for ring geometry & sector hit-testing
│   ├── workshop.ts              # Ring Workshop catalog, custom workspaces & localStorage persistence
│   ├── WorkshopPanel.tsx        # Interactive drag-and-drop Ring Workshop UI
│   ├── SummaryPanel.tsx         # AI Summary panel (second window), state in summary.ts
│   └── DebugOverlay.tsx         # Cursor & angle diagnostics (VITE_DEBUG_DROP=1)
└── src-tauri/                   # Backend (Rust + Tauri v2)
    ├── build.rs                 # Registers app commands in Tauri's ACL manifest; builds flipfile-ai (macOS)
    ├── intelligence/main.swift  # flipfile-ai: Apple Intelligence (FoundationModels) helper process
    ├── capabilities/            # Least-privilege permissions for the WebView
    └── src/
        ├── lib.rs               # Tauri setup, plugins, commands, window sizing
        ├── convert/             # Multi-threaded conversion pipeline
        │   ├── mod.rs           #   Batch scheduling, progress channel & panic catching
        │   ├── input.rs         #   Format detection, image/RAW decoding & EXIF orientation
        │   ├── encode.rs        #   Image encoders (JPG, PNG, WEBP, AVIF, HEIC, ICO, ICNS, SVG, TGA, QOI, EXR, HDR, PPM)
        │   ├── tools.rs         #   Photo tools (Remove BG, byte-budget compress, transform, square, animated GIF)
        │   ├── office_extra.rs  #   Docs, e-books & data tables (MD, TXT, HTML, RTF, EPUB, ODT, CSV, TSV, JSON, YAML, XML, SQL)
        │   ├── media.rs         #   Audio & Video conversion (MP3, WAV, FLAC, M4A, MP4, MOV, WEBM, MKV, GIF, etc.)
        │   ├── pdf.rs           #   PDF ↔ images & multi-image PDF merging
        │   ├── word.rs          #   Word ↔ PDF & OCR → Word
        │   ├── clean.rs         #   Lossless EXIF/GPS/metadata stripping
        │   ├── text.rs          #   Readable text of any document (incl. OCR) for AI summaries
        │   ├── memory.rs        #   System RAM budget & concurrency limiter
        │   ├── output.rs        #   Collision-safe output path generation
        │   ├── tests.rs         #   Unit test suite
        │   ├── stress.rs        #   Hostile-input & boundary stress suite
        │   └── bench.rs         #   Performance & peak-RSS benchmark suite
        ├── office/              # Built-in DOCX ↔ PDF (Typst), PDF → DOCX & XLSX → PDF engines
        ├── pdf/                 # PDFium renderer & minimal PDF writer
        ├── platform/            # Native OS integrations (ImageIO/WIC, Vision OCR/BG, Liquid Glass, Tray)
        ├── summary/             # AI summaries: jobs, the flipfile-ai helper, the panel window, Markdown
        └── state/               # Managed state (grants.rs, history.rs, settings.rs)
```

---

## How to Add a New Format or Tool

1. **Frontend Ring & Catalog:**
   * Register the format/tool ID and slice label in [`src/dropTarget.ts`](src/dropTarget.ts) (in the appropriate Category Room sub-ring) and [`src/workshop.ts`](src/workshop.ts) (so users can drag it into custom rings in the Ring Workshop).
2. **Backend Validation & Dispatch:**
   * Add the format identifier to the allowed targets list in [`src-tauri/src/convert/mod.rs`](src-tauri/src/convert/mod.rs) and route it to the appropriate converter module (`encode.rs`, `tools.rs`, `office_extra.rs`, `media.rs`, etc.).
3. **Output Naming:**
   * Ensure [`src-tauri/src/convert/output.rs`](src-tauri/src/convert/output.rs) assigns the right file extension and suffix without overwriting existing files.
4. **Tests:**
   * Add unit tests in [`src-tauri/src/convert/tests.rs`](src-tauri/src/convert/tests.rs) verifying round-trip conversion and error handling on invalid inputs.

> **Important Security Note:** If you add a new `#[tauri::command]` in `src-tauri/src/lib.rs`, you **must** also add its name to `COMMANDS` in `src-tauri/build.rs` and grant `"allow-<command_name>"` in `src-tauri/capabilities/default.json`. Otherwise Tauri's ACL will block the command at runtime.

---

## Testing & Quality Checks

Before opening a Pull Request, please run the full verification suite:

```bash
# 1. TypeScript strict type-checking & Vite production bundle
npx tsc --noEmit && npm run build

# 2. Rust compiler checks & unit tests
cd src-tauri && cargo check
cd src-tauri && cargo test

# 3. Optional: Hostile-input stress tests & performance benchmarks
npm run stress
npm run bench
```

### Debugging Drop Coordinates

If you are working on ring geometry or window scaling, launch with the debug overlay enabled:

```bash
VITE_DEBUG_DROP=1 npm run tauri dev
```

---

## Pull Request Guidelines

* **Keep PRs focused:** One feature or bug fix per pull request makes review much faster.
* **Preserve offline guarantees:** Do not introduce runtime network calls or unpinned external binaries.
* **Follow code style:**
  * TypeScript/React: 2-space indentation, strict types (no `any`), functional components.
  * Rust: standard `rustfmt` formatting (`cargo fmt --check`) and clean `cargo clippy` output.
* **Update documentation:** If your change adds a user-facing format, shortcut, or behavior, update `README.md` and `CHANGELOG.md` under `[Unreleased]`.

---

## License

By contributing to **flipfile**, you agree that your contributions will be licensed under the [MIT License](LICENSE).

