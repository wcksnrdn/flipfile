# Support & Troubleshooting Guide

Need help using **flipfile**? This guide covers common questions, platform setup tips, and where to get help.

---

## Quick Troubleshooting

### 1. First Launch Warnings ("Can't be verified" or "Windows protected your PC")

Because flipfile builds are distributed directly from GitHub Releases without paid Apple/Microsoft certificate notarization yet, your OS will ask for confirmation on the very first launch:

* **macOS ("flipfile can't be opened / is damaged"):**
  1. Open **System Settings → Privacy & Security**, scroll down, and click **Open Anyway**; *or*
  2. Clear the macOS quarantine attribute in Terminal:
     ```bash
     xattr -cr /Applications/flipfile.app
     ```
* **Windows ("Windows protected your PC"):**
  Click **More info** → **Run anyway**.
* **Linux:**
  * Debian / Ubuntu: `sudo apt install ./flipfile_*.deb`
  * Fedora / RHEL: `sudo dnf install ./flipfile-*.rpm`
  * AppImage: `chmod +x flipfile_*.AppImage && ./flipfile_*.AppImage`

---

### 2. HEIC or AVIF Photos Fail to Open on Windows

On Windows 10/11, flipfile uses Windows Imaging Component (WIC) to decode HEIC and AVIF photos natively. If those codecs are not installed yet, install the free official Microsoft packages from the Microsoft Store:
* **HEIF Image Extensions**
* **AV1 Video Extension** (for `.avif` files)

*(On macOS, HEIC and AVIF work out of the box via Apple ImageIO.)*

---

### 3. Video & Audio Conversions (Extended Formats)

flipfile includes built-in support for common audio/video conversions on macOS via `/usr/bin/afconvert` and `/usr/bin/avconvert`. To unlock **all 23 Video & Music targets** across macOS, Windows, and Linux (including `WEBM`, `MKV`, `AVI`, `FLV`, `OPUS`, `OGG`, `WMA`, `Video → GIF`, and `Compress Video`), install **[FFmpeg](https://ffmpeg.org/)**:

* **macOS (Homebrew):** `brew install ffmpeg`
* **Windows (Winget):** `winget install Gyan.FFmpeg`
* **Linux (APT):** `sudo apt install ffmpeg`

flipfile automatically detects `ffmpeg` in standard system paths (`/opt/homebrew/bin`, `/usr/local/bin`, `PATH`, etc.) with zero configuration.

---

### 4. Customizing Your Ring (Ring Workshop)

If the default ring has too many or too few formats for your workflow:
1. Click the **`Customize`** badge in the center of the flipfile circle (or open the menu bar / system tray icon and click **Customize Ring (Workshop)…**).
2. Choose a ready-made preset (**Default**, **Designer & Web**, **Office & Admin**, **Content Creator**, **Developer & Data**) or create your own workspace.
3. Drag and drop formats or sub-rings from the catalog directly onto any slice of the live ring preview, adjust window size (`S`, `M`, `L`, `XL`), and click **Back to Ring**.

---

### 5. "The PDF engine is missing" during Development

If you are building flipfile from source and see a missing PDFium error, fetch the pinned PDFium library for your platform:

```bash
npm run fetch-pdfium
```

---

## Where to Get Help

* **Found a Bug?**  
  Open a [**Bug Report**](https://github.com/wcksnrdn/flipfile/issues/new?template=bug_report.yml) with your OS version, source/target format, and reproduction steps.
* **Want a New Format or Feature?**  
  Submit a [**Feature Request**](https://github.com/wcksnrdn/flipfile/issues/new?template=feature_request.yml).
* **Found a Security Vulnerability?**  
  Please **do not** open a public issue. Follow our [**Security Policy (SECURITY.md)**](SECURITY.md) to report it privately.

