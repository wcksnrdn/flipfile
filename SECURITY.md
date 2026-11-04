# Security Policy

**flipfile** processes untrusted documents, photos, spreadsheets, and media files from anywhere on your computer. Because file parsers are a classic attack surface, flipfile is engineered with defense-in-depth so that even if a single layer encounters a malformed input, your system and files remain protected.

---

## Supported Versions

We actively maintain and backport security fixes to the latest release series. Please always use the latest version from [GitHub Releases](https://github.com/wcksnrdn/flipfile/releases/latest).

| Version | Supported | Notes |
| --- | :---: | --- |
| `0.3.x` (Latest) | ✅ | Active development & security updates |
| `< 0.3.0` | ❌ | Please upgrade to the latest release |

---

## Reporting a Vulnerability

**Please do not report security vulnerabilities through public GitHub issues, discussions, or pull requests.**

Instead, please report them privately using one of the following channels:

1. **GitHub Private Vulnerability Reporting (Recommended):**  
   Submit a confidential advisory directly at [**Security → Report a vulnerability**](https://github.com/wcksnrdn/flipfile/security/advisories/new).
2. **Direct Maintainer Contact:**  
   Reach out privately to [@wcksnrdn](https://github.com/wcksnrdn) with `[SECURITY] flipfile` in the subject line.

### What to Include in Your Report

To help us triage and reproduce the issue quickly, please include:

* **Component affected** (e.g., `src-tauri/src/state/grants.rs`, `src-tauri/src/office/docx.rs`, `src-tauri/src/convert/encode.rs`, Tauri capabilities, or frontend CSP).
* **Operating system & architecture** (macOS Apple Silicon/Intel, Windows 10/11, or Linux distro).
* **Minimal proof-of-concept (PoC)** file or step-by-step reproduction instructions.
* **Impact assessment** (e.g., path traversal, capability bypass, out-of-bounds read, denial-of-service / decompression bomb).

### Response SLA

| Milestone | Target Timeframe |
| --- | --- |
| Initial acknowledgment | Within **48 hours** |
| Triage & severity validation | Within **5 business days** |
| Fix & coordinated release | Within **14–30 days** (depending on complexity) |

We gladly credit security researchers in the release notes and advisory (unless you prefer to remain anonymous).

---

## Defense-in-Depth Architecture

flipfile enforces strict boundaries between the WebView UI, the Rust native core, and external helper processes:

| Security Layer | Implementation | Protection Provided |
| --- | --- | --- |
| **100% Offline Runtime** | Zero network permissions | No telemetry, no analytics, no remote assets, and no outbound sockets at runtime. Files never leave your machine. |
| **Strict Content Security Policy** | `src-tauri/tauri.conf.json` & `vite.config.ts` | `default-src 'none'`; only bundled local scripts, styles, and fonts are permitted. No `eval`, no inline scripts, and `freezePrototype: true` blocks prototype pollution. |
| **Least-Privilege Tauri ACL** | `src-tauri/build.rs` & `src-tauri/capabilities/default.json` | All custom Rust commands are declared in an `AppManifest` and gated by explicit capability permissions. The UI cannot read arbitrary paths, spawn shells, or open external URLs. |
| **User-Granted File Tokens** | `src-tauri/src/state/grants.rs` | The Rust backend only converts file paths captured directly from native OS drag-and-drop or clipboard events *before* the UI sees them. Even a compromised WebView cannot trick the backend into reading or writing arbitrary files. |
| **Deterministic Output Paths** | `src-tauri/src/convert/output.rs` | Output directories and filenames are computed exclusively in Rust next to the granted input file (or `~/Downloads` for clipboard images) and never overwrite existing files (`_2`, `_3`, …). |
| **Cryptographically Pinned PDFium** | `scripts/fetch-pdfium.mjs` | Both the downloaded archive and extracted `libpdfium` binary are verified against hardcoded SHA-256 digests cross-checked against GitHub build attestations. Any mismatch aborts the build. |
| **Memory & Decompression Guards** | `src-tauri/src/convert/memory.rs` & `office/docx.rs` | Batch conversions enforce a strict RAM budget (25% of system memory), WebP/Square dimensions are capped, ZIP/DOCX/XLSX archives enforce entry size limits against zip bombs, and release builds unwind panics per file (`panic = "unwind"`). |
| **Hardened Subprocess Execution** | `src-tauri/src/office/apps.rs` & `convert/media.rs` | When delegating to Word, Excel, LibreOffice, `ffmpeg`, `afconvert`, or `avconvert`, file paths are passed strictly as OS process arguments or environment variables—never interpolated into shell strings—with strict per-process timeouts and isolated temporary profiles. |

---

## Continuous Security Verification

Every change can be verified against flipfile's automated hostile-input stress suite (`src-tauri/src/convert/stress.rs`), which throws ~90 malformed, truncated, zero-byte, decompression-bomb, hollow-archive, and extreme-dimension inputs against every conversion target (~3,000 combinations):

```bash
cd src-tauri && cargo test
npm run stress
```

Every input must either succeed cleanly or return a human-readable error—never crash, hang, leak memory, or leave behind empty/partial files.

