<div align="center">

<img src="src-tauri/icons/128x128@2x.png" width="120" alt="flipfile icon">

# flipfile

### Drop a file. Point to a format. Done.

**The file converter that lives on your desktop as a single glass circle.**<br>
Photos, PDFs, Word, Excel, PowerPoint, data, video, audio, scans and recordings,<br>
converted, compressed, upscaled and transcribed **right on your computer**.<br>
No uploads. No accounts. No waiting.

[![Download](https://img.shields.io/github/v/release/wcksnrdn/flipfile?label=download&color=1d1d1f&style=for-the-badge)](https://github.com/wcksnrdn/flipfile/releases/latest)
[![Build](https://img.shields.io/github/actions/workflow/status/wcksnrdn/flipfile/ci.yml?branch=main&label=build&style=for-the-badge)](https://github.com/wcksnrdn/flipfile/actions/workflows/ci.yml)
[![Homebrew](https://img.shields.io/badge/Homebrew-wcksnrdn%2Ftap-f5f5f7?logo=homebrew&logoColor=1d1d1f&style=for-the-badge)](https://github.com/wcksnrdn/homebrew-tap)
[![Winget](https://img.shields.io/badge/Winget-wcksnrdn.flipfile-f5f5f7?logo=windows&logoColor=0078d4&style=for-the-badge)](https://github.com/microsoft/winget-pkgs/pull/471276)
[![License: MIT](https://img.shields.io/badge/license-MIT-6e6e73?style=for-the-badge)](LICENSE)

![macOS](https://img.shields.io/badge/macOS-Apple%20Silicon%20%26%20Intel-f5f5f7?logo=apple&logoColor=1d1d1f)
![Windows](https://img.shields.io/badge/Windows-10%20%26%2011-f5f5f7?logo=windows&logoColor=0078d4)
![Linux](https://img.shields.io/badge/Linux-deb%20%C2%B7%20rpm%20%C2%B7%20AppImage-f5f5f7?logo=linux&logoColor=1d1d1f)
![Offline](https://img.shields.io/badge/100%25-offline-30d158)
![Telemetry](https://img.shields.io/badge/telemetry-none-30d158)

<br>

<img src="docs/media/hero.webp" width="100%" alt="flipfile on a Mac desktop: a photo is dragged onto the glass circle, the ring of formats opens, JPG is picked and the converted file appears next to the original">

<br>

[**Download**](#download) &nbsp;·&nbsp; [What's new](#whats-new-in-09) &nbsp;·&nbsp; [Features](#features) &nbsp;·&nbsp; [How to use](#how-to-use) &nbsp;·&nbsp; [Formats](#supported-formats) &nbsp;·&nbsp; [Privacy](#privacy--security) &nbsp;·&nbsp; [FAQ](#faq) &nbsp;·&nbsp; [Website](https://flipfile.satria.cloud)

</div>

<br>

## What's new in 0.9

> [!TIP]
> **Compress anything, and see the size before you let go.** Hold <kbd>⌥ Option</kbd> (<kbd>Alt</kbd> on Windows) while dragging and pick **Light · Balanced · Strong · Extreme** for photos, PDFs, videos and recordings. Each level shows the size it will give (`4.2 MB → 610 KB · −85%`) and what it costs, and a result is never larger than the original.

- **Compress levels** for photos, PDFs (only the pictures inside get smaller; text stays sharp and selectable), MP4 videos and recordings, with a live estimate on every slice. [More ↓](#compress-levels)
- **Notification bubbles** beside the circle: what a compression **gained** (space saved, which upload limit it now fits) and **cost** (format change, softer detail), and what failed and why, file by file, with **Show in Finder / Explorer**.
- **Sharper transcripts**: every word is timed within about a third of a second of its sound, silence and music are cut out before Whisper listens (no made-up sentences over a quiet room), and voice-overs come out punctuated, in readable paragraphs.

Full details in the [CHANGELOG](CHANGELOG.md).

<br>

## Why flipfile

<table>
<tr>
<td width="33%" valign="top">

### 🔒 Private by design
Every conversion runs on your computer. Files never leave it, there is no account and no telemetry, and everything keeps working with the Wi-Fi off.

</td>
<td width="33%" valign="top">

### ⚡ Instant
No upload, no queue, no "your download will start in 30 seconds". Drag onto the circle, point at a format, let go. The result lands next to the original.

</td>
<td width="33%" valign="top">

### 🧰 Does it all
75+ formats and tools: iPhone HEIC, PDF ⇄ Word, Excel and PowerPoint → PDF, data formats, video, audio, OCR, compress, upscale, remove background, subtitles and transcripts.

</td>
</tr>
<tr>
<td width="33%" valign="top">

### ✦ AI on your machine
**Transcribe** (Whisper), **AI Upscale** (Real-ESRGAN + face restoration) and **AI Summary** (Apple Intelligence) run locally. No credits, no cloud, no watermark.

</td>
<td width="33%" valign="top">

### 🖨️ Office that looks like Office
Word, Excel and PowerPoint become PDFs that look like the originals, with **nothing to install**: flipfile has its own typesetting engine, and uses Office or LibreOffice when you have them.

</td>
<td width="33%" valign="top">

### 💸 Free and open source
MIT-licensed, built in public by GitHub Actions, with checksums and signed build provenance for every installer. Mac, Windows and Linux.

</td>
</tr>
</table>

<details>
<summary><b>flipfile vs. online converters</b></summary>

| | **flipfile** | Typical online converters |
|---|---|---|
| Where your files go | Nowhere: converted on your computer | Uploaded to someone's server |
| Account, sign-up | None | Often required for bigger files |
| Free minutes, file-size caps, daily limits | None | Common |
| Queues and waiting | None | Common on free plans |
| Watermarks and ads | None | Common |
| Works offline | Yes | No |
| Price | Free (MIT) | Free tier, then a subscription |

</details>

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

**Compress, with the size up front**<br>
<sub>Hold ⌥ Option / Alt while dragging: pick a level and see the size it will be before you let go.</sub>

<img src="docs/media/compress.webp" width="100%" alt="Holding Option shows the compress ring; the photo comes out 85% smaller">

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

**Works offline**<br>
<sub>Turn the Wi-Fi off: everything still converts, because nothing is ever uploaded.</sub>

<img src="docs/media/offline.webp" width="100%" alt="Switching Wi-Fi off from the menu bar">

</td>
</tr>
</table>

<br>

## Features

| | Feature | What it does | Runs on |
|---|---|---|---|
| 🗜️ | [**Compress levels**](#compress-levels) | Photos, PDFs, videos and recordings, four levels, size shown before the drop | macOS · Windows *(Linux: tray presets)* |
| 🎙️ | [**Transcribe**](#transcribe) | Recordings and videos → SRT/VTT subtitles or TXT, Word, Markdown transcripts, 99 languages | macOS · Windows · Linux |
| 🔍 | [**AI Upscale**](#ai-upscale) | Small, blurry or old photos redrawn 2× or 4×, every face in HD | macOS · Windows · Linux |
| ✦ | [**AI Summary**](#ai-summary) | Any document or recording explained: headline, key points, figures, to-dos; ask it questions | macOS 26 with Apple Intelligence |
| 📄 | [**Office**](#office-word-excel-powerpoint) | Word ⇄ PDF, Excel → PDF, PowerPoint ⇄ PDF/Word/images, anything → slides | macOS · Windows · Linux |
| 🔤 | [**WORD OCR**](#office-word-excel-powerpoint) | Scans and photos of documents → editable Word | macOS · Windows |
| 🖼️ | [**Photos**](#photos) | 19 image formats incl. HEIC, AVIF, EXR, RAW; remove background, transform, square, GIF, ICO/ICNS | macOS · Windows · Linux |
| 🎬 | [**Video & audio**](#video--audio) | MP4, MOV, WEBM, MKV, AVI, FLV, GIF clips; mute, extract audio, 1080p/720p; 11 audio formats | macOS · Windows · Linux *(some need `ffmpeg`)* |
| 🧩 | [**Ring Workshop**](#make-it-yours) | Rearrange the rings, build your own, switch workspace profiles | macOS · Windows · Linux |

### Compress levels

Hold <kbd>⌥ Option</kbd> (macOS) or <kbd>Alt</kbd> (Windows) while dragging. The ring shows four compress levels, **Light · Balanced · Strong · Extreme**, next to the size presets **50% · 1080p · 720p · Original**. Point at a level and the circle says what it will make of your files **before you drop**, for example `4.2 MB → 610 KB` and `−85% · Fine detail softens · max 1920 px`; every level's slice shows its estimated size too.

| Level | Photos | PDFs *(pictures inside; text stays sharp and selectable)* | Videos (MP4) | Recordings (M4A) |
|---|---|---|---|---|
| **Light** | Looks the same | Pictures at 200 dpi | Looks the same, up to 1080p | 160 kbps |
| **Balanced** | Hard to tell apart | Pictures at 150 dpi | Slightly softer, up to 1080p | 128 kbps |
| **Strong** | Fine detail softens, max 1920 px | Pictures at 110 dpi | Softer, 720p | 96 kbps |
| **Extreme** | Visibly soft, max 1280 px | Pictures at 72 dpi | Blocky in motion, 480p | 48 kbps mono |

- **Measured, not guessed**: photos and PDFs are compressed in memory to get their size; videos and recordings are worked out from their length (`≈`, within about 5% for videos on a Mac).
- **Never larger than the original**: a file that can't get smaller is copied and marked "Already compact". Results are saved next to the original as `name_compressed.ext`.
- **Your format where it helps**: JPG, WebP, AVIF and HEIC stay in their format; PNG and TIFF stay lossless at Light and become JPG (WebP when transparent) from Balanced on, and flipfile says so (`PNG → JPG`).
- **Built in on macOS and Windows**: videos and recordings are re-encoded by `ffmpeg` when installed, otherwise by AVFoundation on macOS (hardware encoder, a 47 s 1080p video in 2–8 s) or Media Foundation on Windows; on Linux they need `ffmpeg`. Portrait videos stay portrait.
- **A bubble sums it up** after the drop: sizes, the saving, the **gains** (space saved, upload and attachment limits it now fits) and the **costs**, with a button to show the result.

> On Windows, holding Alt while dragging from File Explorer normally means "create shortcut"; flipfile always reads the dragged files and never moves, links or changes the originals. On Linux, set Resize / Quality in the tray menu instead.

### Transcribe

Drop a lecture, a meeting, a WhatsApp voice note or a video on **Music → Transcribe** (or **Video → Tools → Subtitles**) and flipfile writes down what is said, in the language it was said in.

- **Subtitles**: **SRT** or **VTT**, named after the video (`lecture.mp4` → `lecture.srt`) so players pick them up straight away. A cue per sentence, long ones cut after a comma, never more than two lines and seven seconds.
- **Transcripts**: **TXT**, **Word** or **Markdown**, in short paragraphs with a timestamp that points within about a third of a second of each paragraph's first word (`meeting_transcript.docx`).
- **Only the speech**: silence, music and noise are cut out first (Silero VAD), so a quiet minute or a song never turns into made-up sentences.
- **Any recording**: MP3, M4A, WAV, FLAC, OGG and Opus voice notes, AIFF, CAF, and the sound of MP4, MOV, MKV and WebM, read by flipfile itself; other formats through the system or `ffmpeg`. Up to 5 hours.
- **99 languages**, detected by themselves (Indonesian, English, Javanese, Malay, Japanese, …). It writes what it hears, so names and jargon can come out spelled by ear.
- **Then the gist**: on a Mac with Apple Intelligence, drop the recording on **AI ✦** to have it transcribed and summarised in one go.
- **Private and quick**: OpenAI's [Whisper](https://github.com/openai/whisper) `small` model (about 190 MB, part of flipfile) runs through [whisper.cpp](https://github.com/ggml-org/whisper.cpp): on the GPU on Apple silicon, on the processor elsewhere. The circle shows how far it has got.

<sub>macOS 13 Ventura or later, Windows 10 or later, Linux (x86-64 processors need AVX2, from 2013 on).</sub>

### AI Upscale

Drop a small, blurry or old photo on **Upscale 2×** or **Upscale 4×** (*Photo → Tools*) and flipfile redraws it two or four times larger. Faces get their own AI, so people keep their own face.

- **Faces in HD**: each face is found (Vision on the Mac, [YuNet](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet) elsewhere) and drawn by [4xFaceUpSharpLDAT](https://github.com/Phhofm/models/releases/tag/4xFaceUpDAT_Series), an upscaler trained on faces: sharp eyes, lashes, brows, hair and skin, while glasses, jewellery and features stay as they are. Small or damaged faces are first restored by [RestoreFormer++](https://github.com/wzhouxiff/RestoreFormerPlusPlus).
- **Everything else, sharper**: [Real-ESRGAN](https://github.com/xinntao/Real-ESRGAN) brings back natural detail; edges and text come out crisp, JPEG blocks and noise disappear.
- **Old photos too**: a picture that is large but blurry is recognised and read at the size its detail really has, so it comes back sharper, not just bigger.
- **Your format**: `photo_2x.jpg` stays a JPG, `logo_4x.png` keeps its transparency. Results up to **200 MP** (a 50 MP photo at 2×, a 12.5 MP one at 4×).
- **On your hardware**: the Neural Engine on a Mac, any DirectX 12 GPU on Windows (DirectML), the processor on Linux. flipfile measures how quick your computer is and picks the detailed model only where it finishes in reasonable time.

It draws plausible detail rather than recovering what the camera never captured.

### AI Summary

No time to read a 40-page report? Drop it on **AI ✦** in the lobby. A glass panel opens beside the circle and **Apple Intelligence** explains it while you watch: what kind of document it is, a headline, the one thing to know, key points, key figures (amounts, dates, deadlines), to-dos with who and when, and its topics.

- **Any document**: PDF, Word, PowerPoint, Excel, Markdown, web pages, e-books, scans and photos of text, and recordings and videos (transcribed first).
- **Long ones too**: read part by part, with its notes shown as it goes; a book is condensed to its most telling passages first.
- **Ask it anything**: type a question or tap a suggested one, and get an answer from the document itself (or a plain "the document doesn't say").
- **Your language**: written in the document's own language; one click translates it into your Mac's language and back.
- **Keep it**: copy (`⌘C`) or save next to the original as `name_summary.md` (`⌘S`).

<sub>Needs a Mac with Apple silicon, macOS 26 or later and Apple Intelligence turned on. Elsewhere the slice simply isn't there.</sub>

### Office: Word, Excel, PowerPoint

<img align="right" src="docs/media/excel-to-pdf.webp" width="44%" alt="A profit and loss statement printed from Excel by flipfile, with Rupiah accounting formats and a double-underlined total">

- **Word → PDF** that paginates like Word, **PDF → Word** you can edit (real tables, columns, headers and footers, live page numbers).
- **Excel → PDF** the way Excel prints it: page setup, repeated title rows, merged cells, logos, and **numbers exactly as Excel shows them**, including accounting and Rupiah formats, in your region's separators.
- **PowerPoint → PDF / images / Word handout** with speaker notes, and **anything → PowerPoint**: PDFs, photos, documents and tables become slides.
- **WORD OCR**: scans and photos of documents become editable Word, read by your computer's own text recognition, with the scan kept behind the text so stamps and signatures stay.
- **Documents and data**: Markdown, TXT, HTML, RTF, EPUB, ODT; Excel, CSV, TSV, JSON, YAML, XML, SQL and Markdown tables, in any direction.

Microsoft Office or LibreOffice are used when installed; otherwise flipfile's own engine (built on [Typst](https://typst.app) and PDFium) does it. **Nothing to install either way.** [Engine details ↓](#office-in-detail)

<br clear="right">

### Photos

- **19 formats**, read and write: JPG, PNG, WebP, HEIC, AVIF, TIFF, BMP, GIF, ICO, ICNS, TGA, EXR, HDR, QOI, PPM, SVG; read PSD, JPEG 2000 and camera RAW.
- **Remove BG** *(macOS 14+)*: the subject cut out to a transparent PNG by macOS Vision, offline.
- **Compress to a target** (1 MB · 500 KB · 200 KB · 100 KB) or **losslessly**, **transform** (rotate, flip, grayscale), **Square 1:1** (white behind JPG/BMP, transparent otherwise), **animated GIF** from several photos, **Slides** from photos, **Clean EXIF** (GPS and camera data removed, losslessly for JPG and PNG).
- **Colours stay right**: Display P3 photos (iPhone, Mac screenshots) are converted to sRGB so they look the same everywhere.

### Video & audio

- **Video**: MP4, MOV, WEBM, MKV, AVI, FLV, and **GIF clips**; **Mute**, **extract audio**, **1080p / 720p**, **Compress**, **Subtitles**.
- **Audio**: MP3, WAV, M4A (AAC), FLAC, OGG, AIFF, OPUS, AAC, CAF, ALAC, WMA; drop a video on any audio format to pull out its soundtrack.
- **Built in where the system allows**: `ffmpeg` is used when present (every format and tool). Without it, macOS converts WAV, M4A, AAC, ALAC, FLAC, AIFF, CAF, MP4 and MOV and does Mute, 1080p, 720p and Compress by itself; Windows does MP4, Mute, 1080p, 720p and Compress. Other audio conversions on Windows, and video and audio on Linux, need `ffmpeg`.

### Make it yours

- **Ring Workshop** (<kbd>⌘ ,</kbd> / <kbd>Ctrl ,</kbd>): reorder every ring, or switch to a **Direct Ring** of 3–10 slots that opens right away. Built-in profiles (*Designer & Web*, *Developer & Data*, *Office & Docs*, *Creator & Media*) and your own.
- **Paste** an image or copied files with <kbd>⌘ V</kbd> / <kbd>Ctrl V</kbd>; **show or hide** flipfile from anywhere with <kbd>⌘ ⇧ K</kbd> / <kbd>Ctrl ⇧ K</kbd>.
- **Menu bar / tray**: recent conversions, **Undo** (results go to the Trash, so they can be restored), default resize and quality.
- **One-click updates**, signed and verified, from inside the app.
- **Native glass**: Liquid Glass on macOS 26, a blurred glass circle elsewhere.

<br>

## Download

Get the latest version from **[Releases](https://github.com/wcksnrdn/flipfile/releases/latest)** (about 310 MB: the AI models for upscaling and transcription are most of it).

| Your computer | Install |
|---|---|
| **macOS** (Apple silicon & Intel) | `brew install wcksnrdn/tap/flipfile` |
| **Windows** 10 / 11 | `winget install wcksnrdn.flipfile` · or `flipfile_x.y.z_x64-setup.exe` |
| Mac with Apple silicon (M1–M4) | `flipfile_x.y.z_aarch64.dmg` |
| Mac with Intel | `flipfile_x.y.z_x64.dmg` |
| **Linux** | Debian/Ubuntu `.deb` · Fedora `.rpm` · any distro `.AppImage` |

Or in one line:

```bash
# macOS & Linux
curl -fsSL https://raw.githubusercontent.com/wcksnrdn/flipfile/main/scripts/install.sh | sh
```

```powershell
# Windows 10 / 11 (PowerShell)
irm https://raw.githubusercontent.com/wcksnrdn/flipfile/main/scripts/install.ps1 | iex
```

<details>
<summary><b>First launch: "can't be verified" or "Windows protected your PC"</b></summary>

flipfile isn't notarized by Apple or code-signed for Windows yet (that needs paid certificates), so the first launch needs one extra click:

- **macOS**: open flipfile, click **Done**, then go to **System Settings → Privacy & Security** and click **Open Anyway**. Only once.
- **Windows**: click **More info → Run anyway**.
- **Linux**: `sudo apt install ./flipfile_*.deb`, `sudo dnf install ./flipfile-*.rpm`, or `chmod +x` the AppImage and run it.

</details>

<details>
<summary><b>Verify your download</b></summary>

The warning above means no paid certificate yet, not that anything was flagged. You can check the file yourself:

- **Build provenance**: every installer is built by GitHub Actions from the tagged commit and carries a signed attestation. `gh attestation verify flipfile_x.y.z_aarch64.dmg --repo wcksnrdn/flipfile` shows the workflow and commit that made it, and fails if the file was changed.
- **Checksums**: compare against `SHA256SUMS.txt` on the release page with `shasum -a 256 -c SHA256SUMS.txt --ignore-missing` (Windows: `certutil -hashfile <file> SHA256`).
- **Offline test**: turn off Wi-Fi; every feature still works.

Details in [SECURITY.md](SECURITY.md#verify-your-download).

</details>

<br>

## How to use

1. **Drag** files onto the glass circle. The **lobby** opens around it: **Photo · Office · Video · Music** (and **AI ✦** on Macs with Apple Intelligence).
2. **Point** toward a room to enter it, then toward a format or tool. Each **Back ‹** slice sits right where you came in, so going back is one small move.
3. **Let go.** The result appears next to the original. Click the circle to show it.

Hold <kbd>⌥ Option</kbd> / <kbd>Alt</kbd> while dragging for the **compress levels** and size presets instead.

<details>
<summary><b>Every ring, slice by slice</b></summary>

| Ring | Slices |
|---|---|
| **Lobby** | **Photo** · **Office** · **Video** · **Music** · **AI ✦** *(summary, macOS)* |
| **Photo** | JPG · PNG · WEBP · HEIC · PDF · **Formats •••** · **Tools ✦** · Back ‹ |
| **Photo → Formats** | AVIF · TIFF · GIF · BMP · ICO *(multi)* · ICNS *(macOS)* · **Pro / 3D •••** · Back ‹ |
| **Photo → Formats → Pro / 3D** | JPEG · TGA *(game)* · EXR *(32f HDR)* · HDR *(RGBE)* · QOI *(fast)* · PPM *(netpbm)* · SVG *(embed)* · Back ‹ |
| **Photo → Tools** | **Compress** *(target size)* · **Remove BG** *(macOS)* · **Upscale 2×** · **Upscale 4×** · **Transform** · **Square 1:1** · **GIF anim** · **Slides** *(pptx)* · **Clean EXIF** · Back ‹ |
| **Photo → Tools → Compress** | Auto · 1 MB max · 500 KB max · 200 KB max · 100 KB max · Lossless · Back ‹ |
| **Photo → Tools → Transform** | Rotate 90° › · Rotate ‹ 90° · Rotate 180° · Flip horiz · Flip vert · B&W gray · Back ‹ |
| **Office** | PDF · WORD *(docx)* · **WORD OCR** · EXCEL *(xlsx)* · **SLIDES** *(pptx)* · CSV · **Docs •••** · **Data •••** · Back ‹ |
| **Office → Docs** | MD · TXT · HTML · RTF · EPUB · ODT · Back ‹ |
| **Office → Data** | JSON · TSV · YAML · XML · SQL *(insert)* · MD *(table)* · Back ‹ |
| **Video** | MP4 · MOV · WEBM · MKV · AVI · **GIF** *(clip)* · **Tools ✦** · Back ‹ |
| **Video → Tools** | **Compress** · **Mute** · **Audio** *(extract)* · **1080p** · **720p** · FLV · **Subtitles ✦** · Back ‹ |
| **Music** | MP3 · WAV · M4A *(AAC)* · FLAC · OGG · **More •••** · **Transcribe ✦** · Back ‹ |
| **Music → More** | AIFF · OPUS · AAC · CAF · ALAC *(lossless)* · WMA · Back ‹ |
| **Transcribe** / **Subtitles** | SRT *(subtitles)* · VTT *(web subs)* · TXT *(transcript)* · WORD *(transcript)* · MD *(notes)* · Back ‹ |
| **⌥ / Alt ring** | **Light** · **Balanced** · **Strong** · **Extreme** · 50% · 1080p · 720p · Original |

The ring only switches when you *enter* a slice, so it never flips back and forth while you hold still.

</details>

<details>
<summary><b>Keyboard shortcuts</b></summary>

| Shortcut | Action |
|---|---|
| <kbd>⌘ ⇧ K</kbd> / <kbd>Ctrl ⇧ K</kbd> | Show / hide flipfile from anywhere |
| <kbd>⌘ ,</kbd> / <kbd>Ctrl ,</kbd> | Open / close the Ring Workshop |
| <kbd>⌘ V</kbd> / <kbd>Ctrl V</kbd> | Paste an image or copied files |
| <kbd>⌥ Option</kbd> / <kbd>Alt</kbd> *(hold while dragging)* | Compress levels (with their size) and size presets |
| <kbd>Esc</kbd> | Cancel a paste or close the Ring Workshop |
| <kbd>⌘ Q</kbd> / <kbd>Ctrl Q</kbd> | Quit |

</details>

<details>
<summary><b>Pasting, results, the tray and default sizes</b></summary>

**Pasting**: click the circle, press <kbd>⌘ V</kbd> / <kbd>Ctrl V</kbd>, point at a format and **click**. A copied image is saved in **Downloads** as `Clipboard 2026-10-20 at 14.03.12.png`; copied files are converted next to the originals, exactly like a drop. <kbd>Esc</kbd> or a click on the center cancels.

**After a conversion**: click the circle within a few seconds to show the result; press and drag the circle to move it anywhere. A bubble beside the circle explains failures file by file, and what a compression gained and cost.

**Menu bar / tray**

| Item | What it does |
|---|---|
| Show / Hide flipfile | Bring the window to the front, or hide it |
| Ring Workshop… | Customise the rings and workspace profiles |
| Recent Conversions | The last 10 results; click one to show it. *Clear History* empties the list |
| Undo Last Conversion | Moves the last results to the **Trash / Recycle Bin**, so they can still be restored |
| Resize, Quality | Defaults for every conversion (below) |
| Check for Updates… | Looks for a new release now |
| Quit flipfile | Quit the app |

On macOS the icon adapts to light and dark menu bars; on Windows left-click shows/hides and right-click opens the menu; on Linux the menu is the only entry point.

**Default size and quality for every conversion** (tray menu):

| Resize | Quality *(JPG, AVIF, PDF)* |
|---|---|
| Original Size *(default)* | Best (95%) |
| 50% | High (90%) *(default)* |
| Max 3840 px (4K) | Balanced (80%) |
| Max 1920 px (Full HD) | Smaller File (65%) |
| Max 1280 px (HD) | |
| Max 800 px | |

When changed, the circle shows a small badge such as `1080p · 80%`. Images are never enlarged. **Size presets** in the ⌥ / Alt ring resize a photo in its own format when dropped on directly, or point at one, release the key, then drop on a format to resize *and* convert.

</details>

<br>

## Supported formats

Over **75 formats, tools and conversions**, all processed on your machine.

<details open>
<summary><b>📸 Photos & images</b></summary>

| Format | Extensions | Read | Write | Notes |
|---|---|:---:|:---:|---|
| **JPEG / JPG** | `.jpg`, `.jpeg` | ✓ | ✓ | Full colour resolution at High/Best; MS-SSIM tuning for size-first qualities |
| **PNG** | `.png` | ✓ | ✓ | Lossless; multi-core `oxipng` optimisation on Compress & Lossless |
| **WebP** | `.webp` | ✓ | ✓ | Lossy (follows Quality) with screenshot tuning, or lossless |
| **HEIC / HEIF** | `.heic`, `.heif` | ✓ | ✓ *(macOS)* | Display P3 / sRGB; hardware encoding on macOS |
| **AVIF** | `.avif` | ✓ | ✓ | AV1 compression; macOS's encoder, rav1e elsewhere |
| **TIFF** | `.tiff`, `.tif` | ✓ | ✓ | Deflate lossless on Compress |
| **BMP** | `.bmp` | ✓ | ✓ | 24-bit when there's no alpha |
| **GIF** | `.gif` | ✓ | ✓ | Single frame; several photos → animated GIF |
| **Windows ICO** | `.ico` | ✓ | ✓ | 16–256 px in one icon |
| **Apple ICNS** | `.icns` | — | ✓ | 16–1024 px retina slices |
| **SVG** | `.svg` | — | ✓ | SVG wrapping lossless PNG data |
| **TGA** | `.tga` | ✓ | ✓ | Game & VFX textures, 24/32-bit |
| **OpenEXR** | `.exr` | ✓ | ✓ | 32-bit float HDR |
| **Radiance HDR** | `.hdr` | ✓ | ✓ | RGBE HDR |
| **QOI** | `.qoi` | ✓ | ✓ | Fast lossless |
| **Netpbm PPM** | `.ppm` | ✓ | ✓ | Portable Pixmap |
| **Photoshop PSD** | `.psd` | ✓ *(OS codec)* | — | Flattened image |
| **JPEG 2000** | `.jp2`, `.j2k` | ✓ *(OS codec)* | — | |
| **Camera RAW** | `.dng`, `.cr2`, `.cr3`, `.nef`, `.arw`, `.orf`, `.rw2`, `.raf` | ✓ *(OS codec)* | — | Canon, Nikon, Sony, Apple, Fuji, Olympus, Panasonic |

</details>

<details>
<summary><b>✨ Photo tools</b></summary>

| Tool | Slice | What it does |
|---|---|---|
| **Compress levels** | ⌥ / Alt ring | Light · Balanced · Strong · Extreme, with the size shown before the drop |
| **Target-size compress** | `1 MB` · `500 KB` · `200 KB` · `100 KB` | Fits strictly under the size, lowering quality and then resolution as needed |
| **Lossless / Auto** | `Lossless` · `Auto` | `oxipng` / lossless WebP / Deflate without changing a pixel, or an automatic smaller file |
| **AI Upscale** | `Upscale 2×` · `Upscale 4×` | Real-ESRGAN with face restoration, same format, up to 200 MP |
| **Remove background** | `Remove BG` *(macOS 14+)* | Subject on a transparent PNG via macOS Vision |
| **Transform** | `Rotate` · `Flip` · `B&W` | Rotate 90°/180°, flip, grayscale, in the file's own format |
| **Square 1:1** | `Square 1:1` | Pads to a square without cropping (white behind JPG/BMP, transparent otherwise) |
| **Animated GIF** | `GIF anim` | Several photos → one looping GIF |
| **Slides** | `Slides` | One photo → a 16:9 slide; several → one presentation |
| **Clean metadata** | `Clean EXIF` | Removes EXIF, GPS, XMP, IPTC and comments; lossless for JPG and PNG, colour profiles kept |

</details>

<details>
<summary><b>📄 Documents & Office</b></summary>

| Format | Extensions | Read | Write | Notes |
|---|---|:---:|:---:|---|
| **PDF** | `.pdf` | ✓ | ✓ | Built-in Typst & PDFium; → editable Word, slides or images (200 dpi) |
| **Word** | `.docx` | ✓ | ✓ | Layout, tables, styles, images; → PDF, PPTX, MD, TXT, HTML, RTF, EPUB, ODT |
| **WORD OCR** | scans & photos → `.docx` | ✓ | ✓ | macOS Vision / Windows OCR, scan kept as background |
| **Legacy Word** | `.doc`, `.dot` | ✓ | — | Via Word or LibreOffice when installed |
| **PowerPoint** | `.pptx`, `.ppsx`, `.pptm`, `.potx` | ✓ | ✓ | → PDF, images, Word handout, outlines; anything → slides |
| **Legacy PowerPoint** | `.ppt`, `.pps`, `.pot`, `.odp` | ✓ | — | Via PowerPoint or LibreOffice (`.odp` outlines built in) |
| **Markdown / TXT / HTML / RTF / EPUB / ODT** | `.md`, `.txt`, `.html`, `.rtf`, `.epub`, `.odt` | ✓ | ✓ | Any direction, and → Word, PDF, slides |
| **AI Summary** *(macOS)* | any document, scan, recording | ✓ | `.md` | Apple Intelligence on this Mac |

</details>

<details>
<summary><b>📊 Spreadsheets & data</b></summary>

| Format | Extensions | Read | Write | Notes |
|---|---|:---:|:---:|---|
| **Excel** | `.xlsx`, `.xlsm` | ✓ | ✓ | Prints every sheet to PDF with Excel's number formats |
| **Legacy Excel & Calc** | `.xls`, `.xlt`, `.ods` | ✓ | — | Via Excel or LibreOffice |
| **CSV / TSV** | `.csv`, `.tsv`, `.tab` | ✓ | ✓ | Separator detected; leading zeros kept |
| **JSON / YAML / XML** | `.json`, `.yaml`, `.yml`, `.xml` | ✓ | ✓ | Records ⇄ tables |
| **SQL** | `.sql` | — | ✓ | `CREATE TABLE` + `INSERT INTO` from any table |
| **Markdown tables** | `.md` | ✓ | ✓ | GitHub-flavoured |

</details>

<details>
<summary><b>🎬 Video, 🎵 audio & 🎙️ speech</b></summary>

| Format / tool | Read | Write | Notes |
|---|:---:|:---:|---|
| **MP4** (H.264 + AAC), **MOV**, **WEBM** (VP9 + Opus), **MKV**, **AVI**, **FLV** | ✓ | ✓ | |
| **GIF clip** | ✓ | ✓ | 12 fps, Lanczos-scaled |
| **Compress** (levels or Video → Tools), **Mute**, **Audio extract**, **1080p**, **720p** | ✓ | ✓ | Mute and Compress built in on macOS and Windows |
| **MP3**, **WAV**, **M4A (AAC)**, **FLAC**, **OGG**, **AIFF**, **OPUS**, **AAC**, **CAF**, **ALAC**, **WMA** | ✓ | ✓ | Drop a video to extract its soundtrack |
| **Subtitles / transcripts** | recordings & videos | SRT · VTT · TXT · DOCX · MD | Whisper `small`, 99 languages |

*`ffmpeg` is used when installed (every format and tool). Without it: macOS handles WAV, M4A, AAC, ALAC, FLAC, AIFF, CAF, MP4, MOV, Mute, 1080p, 720p and Compress; Windows handles MP4, Mute, 1080p, 720p and Compress; Linux needs `ffmpeg` for video and audio. Subtitles and transcripts read the sound themselves on every system.*

</details>

<details>
<summary><b>Where results are saved</b></summary>

flipfile **never overwrites** a file. If a name is taken, `_2`, `_3`, … is appended.

| Action | Result |
|---|---|
| Convert `photo.png` to JPG | `photo_converted.jpg` (next to the original) |
| Several images → PDF / PowerPoint | `firstimage_merged.pdf` / `firstimage_merged.pptx` |
| Word / Excel / PowerPoint → PDF | `report_converted.pdf` |
| PDF → Word | `report_converted.docx` |
| Multi-page PDF or Word → PNG | folder `document_pages/` with `page-01.png`, … |
| Compress levels, target size, lossless | `photo_compressed.jpg` · `photo_200kb.jpg` · `photo_lossless.png` |
| AI upscale | `photo_2x.jpg` / `photo_4x.jpg` |
| Remove background / clean metadata | `photo_nobg.png` / `photo_clean.jpg` |
| Rotate / flip / grayscale / square | `photo_rotated.jpg` · `photo_flipped.jpg` · `photo_grayscale.jpg` · `photo_square.jpg` |
| Several images → animated GIF | `firstimage_animated.gif` |
| Video → GIF / Mute / 1080p | `clip_animated.gif` · `clip_muted.mp4` · `clip_1080p.mp4` |
| Transcribe → SRT / VTT / TXT / Word / MD | `clip.srt` · `clip.vtt` · `clip_transcript.txt` · `clip_transcript.docx` · `clip_transcript.md` |
| AI Summary | `report_summary.md` |
| Pasted clipboard image | `~/Downloads/Clipboard 2026-10-20 at 14.03.12.png` |

</details>

<details>
<summary><b>Platform notes</b></summary>

| | macOS | Windows | Linux |
|---|---|---|---|
| HEIC / AVIF input | Built in (ImageIO) | Free **HEIF Image Extensions** (and **AV1 Video Extension** for AVIF) from the Microsoft Store; flipfile tells you if they're missing | Not supported yet |
| HEIC output | ✓ | — | — |
| ⌥ / Alt compress ring | ✓ | ✓ | Tray menu presets |
| Video without `ffmpeg` | MP4, MOV, Mute, 1080p, 720p, Compress | MP4, Mute, 1080p, 720p, Compress | Needs `ffmpeg` |
| Audio without `ffmpeg` | WAV, M4A, AAC, ALAC, FLAC, AIFF, CAF | Compress levels only | Needs `ffmpeg` |
| Remove BG | macOS 14+ | — | — |
| WORD OCR | Vision | Windows.Media.Ocr | — |
| AI Summary | macOS 26 + Apple Intelligence | — | — |
| AI Upscale | Neural Engine | DirectX 12 GPU (DirectML) | CPU |
| Transcribe | GPU on Apple silicon | CPU | CPU (AVX2) |
| Glass effect | Native Liquid Glass (macOS 26+), blur before | CSS glass | CSS glass |

</details>

<br>

## Privacy & security

flipfile works **offline**: files never leave your computer, there is no account and no telemetry. The only network request is the update check, which reads `latest.json` from this repo's GitHub Releases and sends nothing about you or your files; an update is installed only if it carries the maintainer's signature. AI summaries are written by Apple Intelligence's **on-device** model, and transcription and upscaling run on bundled models, so nothing is uploaded there either.

<details>
<summary><b>Stored data, in detail</b></summary>

Two small files in the app data folder:

| File | Contents |
|---|---|
| `history.json` | Paths of your last 50 conversions (for *Recent Conversions* and *Undo*) |
| `settings.json` | Your Resize and Quality choices |

| OS | App data folder |
|---|---|
| macOS | `~/Library/Application Support/com.satria.flipfile/` |
| Windows | `%APPDATA%\com.satria.flipfile\` |
| Linux | `~/.local/share/com.satria.flipfile/` |

Your Ring Workshop layout and workspace profiles live in the app window's own storage (`localStorage`) on your computer. Pasted clipboard images are first written to the app's cache folder; that copy is replaced on the next paste and cleared on every launch. *Clear History* in the tray menu empties `history.json`. Files dragged over the window may be read to estimate compressed sizes, but are never converted or written next to before you drop them.

</details>

<details>
<summary><b>Security, layer by layer</b></summary>

flipfile opens files from anywhere, so it is built to stay safe even if something goes wrong in one layer.

| Layer | Protection |
|---|---|
| **Content Security Policy** (`tauri.conf.json`) | The UI may only run the app's own script, style and font files. No `eval`, no inline scripts or event handlers, no remote images, frames or network requests. `default-src 'none'`. |
| **Dev server CSP** (`vite.config.ts`) | `tauri dev` gets the same policy from the Vite dev server (minus inline scripts/styles, which hot reload needs). |
| **Frozen prototypes** | `freezePrototype` blocks prototype-pollution attacks in the webview. |
| **Least-privilege capabilities** (`capabilities/`) | The UI can only listen to events, read the scale factor, move the window, reveal a file and call flipfile's own commands. The summary panel and the notification bubble can only do their own job. Everything else is refused. |
| **App commands under the ACL** (`build.rs`) | flipfile's own commands are declared in an `AppManifest`, so capabilities apply to them like to plugin commands. |
| **User-granted files only** (`src-tauri/src/state/grants.rs`) | The backend converts only files the user actually dropped or pasted, recorded from the native event before the UI sees it. The output folder is decided in Rust, never by the UI. |
| **Input checks** | Pasted image names are sanitised; window geometry values are range-checked. |
| **Memory-safe decoding** | Images are decoded in Rust; HEIC/AVIF by the operating system; PDFs by PDFium, pinned to a known release. |
| **Pinned downloads** (`scripts/fetch-*.mjs`) | PDFium, the AI models and ONNX Runtime are pinned by SHA-256 and re-checked on every build; any mismatch stops the build. |
| **Office apps run safely** (`src-tauri/src/office/apps.rs`) | File paths are passed as arguments, never pasted into script text; every run has a time limit; LibreOffice gets a private, temporary profile. Word documents are read with size limits (no zip bombs), external links are never followed, and text is escaped before layout. |
| **AI in its own process** (`src-tauri/intelligence/main.swift`) | Apple Intelligence runs in a small helper, `flipfile-ai`, that gets the document's text and nothing else. A model that hangs or crashes cannot take the app with it. |
| **Offline** | No telemetry and no remote content; the webview can only talk to the app itself. |

Download flipfile **only from this repository's GitHub Releases** (or the Homebrew tap and Winget, which point to them). To report a vulnerability, please don't open a public issue; see [SECURITY.md](SECURITY.md).

</details>

<br>

## FAQ

<details>
<summary><b>Is flipfile really free?</b></summary>

Yes. It is open source under the MIT license: no trial, no Pro tier, no watermark, no account.

</details>

<details>
<summary><b>Does it upload my files anywhere?</b></summary>

No. Every conversion, compression, upscale, transcription and summary runs on your computer. Turn off the Wi-Fi and everything still works. The only request it makes is the update check against GitHub Releases.

</details>

<details>
<summary><b>Why is the download about 310 MB?</b></summary>

It carries its own AI models so nothing has to be downloaded later or sent to a server: about 190 MB for Whisper (transcription) and about 140 MB for upscaling and face restoration, plus PDFium and the Typst layout engine for Office documents. On Windows and Linux it also bundles ONNX Runtime.

</details>

<details>
<summary><b>Do I need Microsoft Office, LibreOffice or ffmpeg?</b></summary>

Not for documents: flipfile has its own engines for Word, Excel and PowerPoint (old `.doc`/`.xls`/`.ppt` files need Office or LibreOffice). For video and audio it depends on the system: macOS handles the common formats itself, Windows handles MP4, Mute, 1080p, 720p and Compress, and Linux needs `ffmpeg` (`sudo apt install ffmpeg`). Installing `ffmpeg` unlocks every format and tool everywhere. Transcribe never needs it for the usual formats.

</details>

<details>
<summary><b>Why does my Mac or PC warn me the first time?</b></summary>

flipfile isn't signed with a paid Apple Developer ID or a Windows code-signing certificate yet, so Gatekeeper and SmartScreen don't recognise the publisher. It isn't flagged as anything; see [First launch](#download) for the one-time click, and [Verify your download](#download) to check the file yourself.

</details>

<details>
<summary><b>Which features need a Mac?</b></summary>

AI Summary needs a Mac with Apple Intelligence (macOS 26, Apple silicon); Remove BG needs macOS 14+; HEIC output is macOS-only. WORD OCR works on macOS and Windows. Everything else, including Transcribe, AI Upscale and compress levels, works on Windows too. On Linux, the documents, photos, Transcribe and AI Upscale work out of the box; video and audio need `ffmpeg`, and the ⌥ / Alt ring isn't there yet (use the tray menu's Resize / Quality).

</details>

<details>
<summary><b>How do I update?</b></summary>

flipfile checks for updates a few seconds after it starts; when one is out, click **Update Now** in the dialog. You can also pick **Check for Updates…** in the tray menu, or run `brew upgrade --cask flipfile` / `winget upgrade wcksnrdn.flipfile`.

</details>

<details>
<summary><b>Something didn't convert. What now?</b></summary>

The bubble beside the circle says what failed and why, file by file. Common fixes are in [Troubleshooting](#for-developers) below; otherwise please [open an issue](https://github.com/wcksnrdn/flipfile/issues) with the file type and your OS.

</details>

<br>

## For developers

<details>
<summary><b>Development: run, build, test</b></summary>

### Prerequisites

- [Node.js](https://nodejs.org) 20+ (developed with 24)
- [Rust](https://rustup.rs) (stable)
- The [Tauri v2 prerequisites](https://tauri.app/start/prerequisites/) for your OS (Xcode Command Line Tools on macOS, WebView2 + MSVC Build Tools on Windows, WebKitGTK and friends on Linux)
- [CMake](https://cmake.org), to build whisper.cpp; on Windows also LLVM (libclang), so whisper.cpp's bindings are generated for Windows
- Internet access the first time, to download PDFium, the AI models and (on Windows and Linux) ONNX Runtime

### Run

```bash
npm install
npm run tauri dev
```

`tauri dev` and `tauri build` first run `npm run fetch-pdfium` and `npm run fetch-models`, which download PDFium into `src-tauri/pdfium/` and the AI models into `src-tauri/models/` (both git-ignored, pinned by SHA-256, skipped when already there). The PDFium version is pinned in `scripts/fetch-pdfium.mjs` to match the `pdfium-render` crate; update both together. To fetch for another architecture, for example an Intel Mac app built on Apple silicon:

```bash
PDFIUM_PLATFORM=mac-x64 npm run fetch-pdfium
```

### Build

```bash
npm run tauri build
```

The installers end up in `src-tauri/target/release/bundle/` (`.app`/`.dmg` on macOS, `.msi`/`.exe` on Windows, `.AppImage`/`.deb`/`.rpm` on Linux). Most of their size is the bundled models: Whisper `small` for transcription (`ggml-small-q5_1.bin`, 190 MB, from the whisper.cpp project on Hugging Face), Silero VAD (about 1 MB), and the upscaling and face models (about 140 MB, from this repo's `models-2` release: Core ML on macOS, ONNX elsewhere; `scripts/upscale-model/` rebuilds them from the original weights). On Windows and Linux, `npm run fetch-onnxruntime` also downloads Microsoft's ONNX Runtime 1.24.4 (with DirectML on Windows) into `src-tauri/onnxruntime/`. To test that engine on a Mac, run with `FLIPFILE_ONNX=1`. Licences are in `src-tauri/models/LICENSES.md`.

### App icon

The icon's source is `src-tauri/icons/app-icon.svg`. After editing it, export a 1024 × 1024 PNG (any SVG renderer, e.g. [resvg](https://github.com/linebender/resvg)) and regenerate every size:

```bash
resvg src-tauri/icons/app-icon.svg app-icon.png -w 1024
npm run tauri icon app-icon.png
rm -rf src-tauri/icons/android src-tauri/icons/ios   # desktop only
```

The macOS menu bar icon has its own source, `src-tauri/icons/tray-template.svg`:

```bash
resvg src-tauri/icons/tray-template.svg src-tauri/icons/tray-template.png -w 44 -h 44
```

### Checks

```bash
npm run build                      # TypeScript (strict) + Vite
cd src-tauri && cargo check        # Rust
cd src-tauri && cargo test         # Rust unit tests
```

The tests cover every output format, EXIF orientation, metadata stripping, PDF merging and rendering, resize/quality, compress levels, transcription, history and settings. `renders_pdf_pages_to_images` needs PDFium from `npm run fetch-pdfium`; `decodes_heic_and_avif_via_imageio` runs only on macOS.

They include stress tests (`src-tauri/src/convert/stress.rs`): about 90 hostile and odd inputs (empty, truncated, random bytes behind every extension, a decompression bomb, hollow Office files, 1 px and 70,000 px images) against every target format, close to 3,000 conversions, each with a time limit. Every file must come back converted or with an error message: never a crash, a hang, a left-over empty file or two outputs with the same name. `npm run stress` adds the slow ones that go through the system engines.

### Benchmarks

```bash
npm run bench                                   # every scenario
FLIPFILE_BENCH=heic,pdf npm run bench           # only scenarios whose name contains one of these
FLIPFILE_BENCH_SAVE=before.tsv npm run bench    # keep the results…
FLIPFILE_BENCH_BASE=before.tsv npm run bench    # …and compare a later run with them (+/- %)
```

Each scenario (12 and 48 MP photos to every common format, HEIC, screenshots, icons, batches of 24 photos, PDF pages, Word, Excel) runs in its own process and reports time, peak memory and output size.

### Reviewing Word ↔ PDF and Excel → PDF output

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

Shows a red dot where flipfile thinks the cursor is, with the raw Tauri position, `devicePixelRatio`, scale factor, converted position, angle, ring and the format that would be used; every drag event is logged with the prefix `[drop-debug]`. Adding `?debug` to the page URL works too.

</details>

<details>
<summary><b>Project structure</b></summary>

```
├── .github/workflows/
│   ├── ci.yml                 # typecheck, build and Rust tests on every push / PR
│   └── release.yml            # installers for every OS, SHA256SUMS and build provenance, on a v* tag
├── Casks/flipfile.rb          # Homebrew cask
├── scripts/
│   ├── fetch-pdfium.mjs       # downloads & SHA-256-verifies PDFium
│   ├── fetch-models.mjs       # downloads & SHA-256-verifies the AI models (upscaling, Whisper, VAD)
│   ├── fetch-onnxruntime.mjs  # downloads & SHA-256-verifies ONNX Runtime (Windows, Linux)
│   ├── upscale-model/         # builds the upscaling models (Real-ESRGAN, RestoreFormer++, FaceUp DAT)
│   ├── install.sh             # one-line installer for macOS & Linux
│   └── install.ps1            # one-line installer for Windows PowerShell
├── src/                       # UI (React + TypeScript)
│   ├── App.tsx                # drag & drop, paste, rings, presets, live estimates, status, progress
│   ├── App.css                # glass look, ring and segment animations
│   ├── dropTarget.ts          # ring layout + sector hit-testing (single source of truth)
│   ├── workshop.ts            # Ring Workshop catalog, workspace profiles & slot persistence
│   ├── WorkshopPanel.tsx      # Ring Workshop editor
│   ├── SummaryPanel.tsx       # AI Summary panel (macOS; summary.ts: its state)
│   ├── ToastBubble.tsx        # notification bubble beside the circle (toast.ts: its messages)
│   ├── UpdateDialog.tsx       # in-app updater dialog (updater.ts: the check and install)
│   └── DebugOverlay.tsx       # optional drop-position diagnostics
└── src-tauri/                 # native side (Rust)
    ├── tauri.conf.json        # window, bundle, identifier (com.satria.flipfile), resources
    ├── build.rs               # declares the app's commands so capabilities apply to them
    ├── capabilities/          # least-privilege permissions per window (default, summary, toast)
    ├── intelligence/          # flipfile-ai, the Apple Intelligence helper (Swift)
    └── src/
        ├── lib.rs             # app setup: plugins, tray, shortcut, state, commands
        ├── toast.rs           # the notification bubble window
        ├── summary/           # AI Summary: reading, chunking, the helper process
        ├── convert/           # conversion pipeline behind `convert_batch`
        │   ├── mod.rs         #   batching, progress, dispatch per file
        │   ├── input.rs       #   detection, decoding with EXIF orientation & ICC → sRGB
        │   ├── encode.rs      #   SIMD resize + image encoders
        │   ├── tools.rs       #   photo tools: remove BG, target-size compress, transform, square, GIF
        │   ├── compress/      #   compress levels: photo, PDF (lopdf), video & audio, live estimates
        │   ├── transcribe/    #   Whisper: audio decoding, VAD, sentences, cues, paragraphs
        │   ├── upscale.rs     #   AI upscaling: tiles with margins, 2× / 4×, alpha
        │   ├── faces.rs       #   face restoration and blending
        │   ├── pdf.rs         #   PDF ⇄ images (streaming page by page)
        │   ├── word.rs        #   Word → PDF/images, OCR → Word
        │   ├── slides.rs      #   PowerPoint ⇄ PDF/images/handout/outlines, anything → .pptx
        │   ├── office_extra.rs #  documents and data conversions
        │   ├── text.rs        #   a file's readable text for AI summaries (OCR, speech too)
        │   ├── media.rs       #   video & audio (ffmpeg, AVFoundation, Media Foundation)
        │   ├── memory.rs      #   RAM budget scheduler for parallel batches
        │   ├── output.rs      #   unique output names, never overwrites
        │   ├── bench.rs       #   `npm run bench`
        │   └── stress.rs      #   `npm run stress`
        ├── pdf/               # streaming PDF writer, PDFium rendering
        ├── office/            # Word ⇄ PDF, Excel → PDF, PowerPoint ⇄ PDF/Word, OCR
        ├── platform/          # OS integration
        │   ├── system_codec.rs #  HEIC/AVIF/PSD/RAW via ImageIO / WIC
        │   ├── background.rs  #   background removal via macOS Vision
        │   ├── upscale.rs     #   Core ML on macOS, ONNX Runtime elsewhere
        │   ├── whisper.rs     #   whisper.cpp: Metal on Apple silicon, CPU elsewhere
        │   ├── faces.rs, yunet.rs # face detection: Vision, else YuNet
        │   ├── tray.rs        #   menu bar / tray
        │   ├── clipboard.rs   #   ⌘V / Ctrl+V
        │   ├── keys.rs        #   Option/Alt state during a drag
        │   └── glass.rs       #   native glass behind the webview (macOS)
        └── state/             # settings, granted files, history
```

</details>

<details>
<summary><b>How it works</b></summary>

**One source of truth for the ring.** `src/dropTarget.ts` defines the rings (items, start angle, sector size) and the geometry. Segment shapes, label positions and hit-testing are all derived from it, so what you see and what gets selected cannot drift apart.

**Sector-based selection.** The cursor is taken relative to the center of the core. Inside the core nothing is selected; outside it, the format whose angular slice contains the cursor wins. On drop the position is evaluated again, and if that lands on nothing, a valid hover from the last 150 ms is used.

**Drop coordinates per OS.** Tauri reports drag positions as `PhysicalPosition`, but the webviews disagree on units:

| OS | Source | Units | Conversion |
|---|---|---|---|
| macOS | `NSDraggingInfo.draggingLocation` | points (logical) | none |
| Windows | `ScreenToClient` | physical pixels | `toLogical(scaleFactor)` |
| Linux | GTK `drag-motion` | logical | none |

**Modifier keys during a drag.** OS drag events carry no modifier keys, and the window usually isn't focused while you drag from Finder or Explorer, so the UI polls a small Rust command (`keys.rs`) every 80 ms while the ring is open (`NSEvent.modifierFlags` on macOS, `GetAsyncKeyState(VK_MENU)` on Windows).

**Live estimates.** While the ⌥ / Alt ring is open, the dragged files are measured in the background: photos and PDFs by compressing them in memory at every level, videos and recordings from their duration and bitrate. Nothing is written until you drop.

**Glass.** CSS `backdrop-filter` can only blur what is inside the webview, not the desktop behind a transparent window, so on macOS `glass.rs` places a native `NSGlassEffectView` (or `NSVisualEffectView` before macOS 26) under the webview, shaped as a circle that grows with the ring.

**Conversions** run on a worker pool sized to your CPU and RAM, off the UI thread, and report progress through a Tauri channel. PDFium renders PDFs page by page, so large documents never sit in memory all at once.

</details>

<details>
<summary><b>Office in detail</b></summary>

<a id="office-in-detail"></a>

**Word → PDF**, best engine first:

| Engine | When | Result |
|---|---|---|
| Microsoft Word | Installed (Windows) | Identical to printing from Word; Word runs invisibly and is closed afterwards (not used on macOS, where Word is sandboxed) |
| LibreOffice | Installed (any OS) | Very close to Word, run invisibly with a private profile |
| **Built-in** | Always | flipfile's own engine reads the document (styles, fonts, lists, tables with merged cells and repeating headers, images, text boxes, headers and footers with page numbers, tab stops) and lays it out with [Typst](https://typst.app), following Word's own rules for line heights, header and footer space, table rows and floating pictures, so page breaks land where Word puts them. SmartArt, charts and tracked-change markup are simplified. |

**PDF → Word** rebuilds the document from the PDF's layout and keeps every page on its own page: ruling lines become real Word tables (merged cells, borders, shading); text keeps its fonts, sizes, styles, alignment, indents and spacing; repeated top and bottom content becomes the header and footer with a live page number; side-by-side blocks stay side by side; pictures stay where they were; coloured bands, boxes and page-size artwork are drawn behind the editable text. On a test set of 30 varied PDFs, every converted document has exactly as many pages as the original. Scanned pages without text stay page images in **WORD**; use **WORD OCR** for their text.

**Excel → PDF** prints every visible sheet with its own page setup (paper, orientation, margins, scale or fit to pages, print area, repeated title rows, page breaks, headers and footers), numbers exactly as Excel formats them (accounting and currency formats, `[Red]` negatives, percentages, dates) in your region's separators, and the sheet's look (column widths, row heights, fonts, fills, borders, merged cells, wrapped text, logos). CSV prints as a table with a bold header row. Charts, shapes and conditional formatting are not drawn by the built-in engine yet.

**PowerPoint** renders slide → layout → master → theme inheritance, theme colours, ~70 preset shapes and custom geometry, gradients and picture fills, groups, cached SmartArt, text styles, bullets, autofit and tables. A deck dropped on **WORD** becomes a printable handout with each slide's picture, title and speaker notes; on **MD / TXT / HTML / RTF / EPUB / ODT** an outline. PDFs, photos, documents and tables dropped on **SLIDES** become presentations. Old `.ppt`/`.pps`/`.pot` files need PowerPoint or LibreOffice.

**WORD OCR** reads scans with macOS Vision (Indonesian and English first) or Windows.Media.Ocr, rebuilds paragraphs, columns and headers like PDF → Word, and keeps the scan, with the text taken out, as the page background so logos, stamps and signatures stay.

</details>

<details>
<summary><b>Troubleshooting</b></summary>

| Problem | What to do |
|---|---|
| "The PDF engine is missing" | Reinstall flipfile. When developing, run `npm run fetch-pdfium`. |
| HEIC photos fail on Windows | Install **HEIF Image Extensions** from the Microsoft Store (plus **AV1 Video Extension** for AVIF). |
| <kbd>⌘ ⇧ K</kbd> / <kbd>Ctrl ⇧ K</kbd> does nothing | Another app has claimed the shortcut. Use the tray icon instead. |
| The ⌥ / Alt ring doesn't appear on Linux | Not supported on Linux yet; set Resize / Quality in the tray menu. |
| "Already compact" after compressing | The file was already as small as that level makes it (common for small PNG screenshots); you get a copy so you can compare. |
| Transcribe says nobody is speaking | Silence, music and noise are cut out first; a recording with no speech gives this message instead of made-up text. |
| A drop picks the wrong format | Run with `VITE_DEBUG_DROP=1` and check that the red dot follows the cursor. Please report the readout. |

</details>

<details>
<summary><b>Contributing</b></summary>

Bug reports, ideas and pull requests are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) and the [Code of Conduct](CODE_OF_CONDUCT.md) first, and see [SUPPORT.md](SUPPORT.md) for where to ask questions.

</details>

<br>

## Acknowledgements

flipfile stands on great open-source work: [Tauri](https://tauri.app), [Typst](https://typst.app), [PDFium](https://pdfium.googlesource.com/pdfium/) (via [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries)), [whisper.cpp](https://github.com/ggml-org/whisper.cpp) and OpenAI's [Whisper](https://github.com/openai/whisper), [Silero VAD](https://github.com/snakers4/silero-vad), [Real-ESRGAN](https://github.com/xinntao/Real-ESRGAN), [RestoreFormer++](https://github.com/wzhouxiff/RestoreFormerPlusPlus), [4xFaceUpSharpLDAT](https://github.com/Phhofm/models), [YuNet](https://github.com/opencv/opencv_zoo), [ONNX Runtime](https://onnxruntime.ai), [Symphonia](https://github.com/pdeljanov/Symphonia), [lopdf](https://github.com/J-F-Liu/lopdf), [oxipng](https://github.com/shssoichiro/oxipng) and [rav1e](https://github.com/xiph/rav1e). Model licences are listed in [`src-tauri/models/LICENSES.md`](src-tauri/models/LICENSES.md).

<br>

<div align="center">

<img src="src-tauri/icons/128x128.png" width="64" alt="">

**flipfile** &nbsp;·&nbsp; Drop. Flip. Done.

[**Download for free**](https://github.com/wcksnrdn/flipfile/releases/latest) &nbsp;·&nbsp; [flipfile.satria.cloud](https://flipfile.satria.cloud)

<sub>[MIT](LICENSE) © wcksnrdn</sub>

<sub>This software is based in part on the work of the Independent JPEG Group.</sub>

</div>
