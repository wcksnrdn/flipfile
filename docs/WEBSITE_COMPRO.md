# Flipfile — Product & Website Documentation (Company Profile / Landing Page Blueprint)

Dokumen ini dirancang khusus sebagai **materi utama pembuatan Website Company Profile / Landing Page Flipfile**, mencakup positioning produk, copywriting (Hero, Feature Grid, Comparison), daftar lengkap **117+ ekstensi input**, **72+ target konversi & tools**, **3.200+ kombinasi konversi**, spesifikasi teknis, serta FAQ.

---

## 1. Brand Identity & Positioning

- **Nama Produk**: **Flipfile**
- **Tagline Utama (Hero Headline)**:
  - *Opsi 1 (Fokus Kecepatan & Privasi)*: **"Drop. Point. Converted. 100% Local File Converter."**
  - *Opsi 2 (Bahasa Indonesia)*: **"Konversi 110+ Format File dalam Satu Gerakan Drag & Drop — 100% Lokal Tanpa Upload."**
  - *Opsi 3 (Developer & Creator Focus)*: **"Satu Lingkaran Kecil di Layar Anda. Lebih dari 3.200 Kombinasi Konversi di Dalamnya."**
- **Sub-headline**:
  > Flipfile adalah aplikasi desktop ultra-ringan (~5 MB) untuk macOS dan Windows yang mengubah foto, dokumen Office, e-book, data developer, video, dan audio secara instan langsung di perangkat Anda. Tanpa cloud, tanpa batas ukuran file, tanpa langganan bulanan.
- **Value Proposition Utama (4 Pilar)**:
  1. **Zero-Click Radial UI**: Tidak perlu membuka jendela aplikasi besar atau mencari menu dropdown. Cukup seret file ke bola kaca mengambang (*floating glass orb*), arahkan ke segmen format, lalu lepaskan.
  2. **100% Offline & Private**: Semua pemrosesan berjalan di CPU/GPU lokal perangkat Anda. Dokumen rahasia perusahaan, kontrak hukum, maupun foto pribadi tidak pernah dikirim ke server mana pun.
  3. **All-in-One 4 Category Rooms**: Empat ruangan khusus (**Photo**, **Office**, **Video**, **Music**) yang mendukung **117 ekstensi file input**, **72 target output & smart tools**, dan **3.200+ kombinasi konversi**.
  4. **Native OS Superpowers & Ultra-Lightweight**: Memanfaatkan akselerasi perangkat keras bawaan OS (Apple Vision AI, ImageIO, CoreAudio/AVFoundation, Windows WIC & WinRT OCR) dalam ukuran aplikasi hanya ~5 MB.

---

## 2. Statistik Utama (Angka Highlight untuk Landing Page)

Gunakan angka-angka ini pada bagian **Stats Counter / Bento Grid** di website:

| Metrik | Angka | Deskripsi Singkat untuk Website |
| :--- | :--- | :--- |
| **Ekstensi File Input** | **117+** | Mulai dari PNG, HEIC, PSD, Camera RAW, PDF, DOCX, XLSX, EPUB, JSON, SQL, hingga MP4 & FLAC |
| **Target Format & Tools** | **72+** | Output format + fitur pintar (AI Remove BG, Target-Size Compress, OCR, Video Mute/Resize) |
| **Pasangan Konversi** | **3.200+** | Kombinasi lintas format (`Input → Output`) di 4 ruangan kategori |
| **Privasi Data** | **100% Lokal** | Nol byte yang diunggah ke internet; aman untuk dokumen NDA & rahasia perusahaan |
| **Ukuran Installer** | **~5 MB** | Dibangun dengan Rust & Tauri — 30x lebih ringan dibanding aplikasi berbasis Electron |

---

## 3. Arsitektur 4 Category Rooms (Fitur Unggulan per Ruangan)

Saat pengguna menyeret file ke dalam Flipfile, lingkaran utama menampilkan 4 ruangan bersih (**Photo**, **Office**, **Video**, **Music**). Mengarahkan kursor ke salah satu ruangan langsung membuka sub-ring format dan peralatan khusus.

### A. Photo Room (Gambar, Desain, 3D/VFX, Icon & Kamera RAW)
Dirancang untuk fotografer, desainer UI/UX, game developer, dan pengguna harian.
- **Format Populer & Modern**: `PNG`, `JPG/JPEG`, `WEBP` (Lossy & Lossless), `HEIC` (Native Apple encoder), `AVIF` (AV1 Image), `GIF`, `TIFF` (Lossless Deflate), `BMP`, `PDF` (gabungkan banyak foto menjadi 1 PDF).
- **App Icons Siap Pakai**:
  - `ICO`: Otomatis menghasilkan file `.ico` multi-resolusi (`16, 32, 48, 64, 128, 256 px`).
  - `ICNS`: Otomatis menghasilkan paket ikon aplikasi macOS `.icns` (`icp4` hingga `ic10` Retina `1024 px`).
- **Pro / 3D / Game & Vector (`photo-pro`)**:
  - `EXR` (OpenEXR 32-bit float HDR), `HDR` (Radiance HDR), `TGA` (Truevision Targa), `QOI` (Quite OK Image), `PPM` (Netpbm), dan `SVG` (vektor wrapper).
- **Dukungan 25+ Kamera RAW & Desain**:
  - Membuka langsung file `.psd` (Photoshop), `.jp2` (JPEG 2000), `.dds`, serta file kamera profesional Canon (`.cr2`, `.cr3`), Nikon (`.nef`, `.nrw`), Sony (`.arw`, `.srf`, `.sr2`), Fujifilm (`.raf`), Adobe (`.dng`), Olympus (`.orf`), Panasonic (`.rw2`), Pentax (`.pef`), Leica (`.rwl`), Hasselblad (`.3fr`, `.fff`), Phase One (`.iiq`).
- **Smart Photo Tools (`photo-tools`, `photo-compress`, `photo-transform`)**:
  - **AI Remove Background (`No BG`)**: Menghapus latar belakang foto menjadi PNG transparan secara instan menggunakan Neural Engine lokal (`VNGenerateForegroundInstanceMaskRequest`).
  - **Target-Size Compression**: Tekan ukuran foto tepat di bawah target **`1 MB`**, **`500 KB`**, **`200 KB`**, **`100 KB`** (melalui pencarian biner kualitas & skala otomatis), atau **`Lossless`** (`oxipng` / lossless WebP).
  - **Clean Metadata (`Clean`)**: Hapus lokasi GPS, EXIF kamera, dan profil tersembunyi tanpa menurunkan kualitas piksel.
  - **1:1 Square Pad (`1:1`)**: Tengah-kan gambar di atas kanvas persegi transparan tanpa memotong (*crop*).
  - **Animated GIF Maker (`GIF+`)**: Drop beberapa foto sekaligus untuk digabungkan menjadi satu animasi GIF.
  - **Transformasi Cepat**: Putar `90°↻`, `90°↺`, `180°`, balik horizontal (`Flip H`), vertikal (`Flip V`), atau ubah ke hitam-putih (`B&W`).

---

### B. Office & Data Room (Dokumen, E-Book, Presentasi & Developer Data)
Dirancang untuk pekerja kantoran, penulis, mahasiswa, analis data, dan software engineer.
- **Konversi Dokumen & PDF Presisi Tinggi**:
  - `DOCX / DOC ⇄ PDF`: Menggunakan mesin typesetting bawaan (Typst) atau Microsoft Word/LibreOffice jika terinstal.
  - `PDF → DOCX` & `PDF/Gambar → DOCX OCR`: Ekstraksi struktur paragraf, tabel, daftar bernomor, serta **OCR lokal** (membaca teks dari PDF hasil scan atau foto dokumen).
- **Sub-ring `Docs` (`office-docs`) — Dokumen, Web & E-Book**:
  - Konversi lintas format antara **`PDF`**, **`DOCX`**, **`ODT`** (OpenDocument Text), **`RTF`** (Rich Text), **`MD`** (Markdown), **`TXT`**, **`HTML`** (lengkap dengan stylesheet bersih), dan **`EPUB`** (membuat e-book `.epub` siap baca di Apple Books / Kindle, atau mengekstrak isi `.epub` ke format lain).
  - Ekstraksi slide presentasi **`PPTX`** & **`ODP`** menjadi `PDF`, `DOCX`, `MD`, `HTML`, `EPUB`, atau `TXT`.
- **Sub-ring `Data` (`office-data`) — Spreadsheet & Developer Data**:
  - Konversi dua arah antara **`XLSX`** (Excel), **`CSV`**, **`TSV`**, **`JSON`** (otomatis mengenali tipe data angka & boolean), **`YAML`**, **`XML`**, **`SQL`** (otomatis men-generate skrip `CREATE TABLE` dan `INSERT INTO` siap jalankan di database), serta **`MD Table`** (tabel Markdown untuk dokumentasi GitHub/Notion).

---

### C. Video Room (Konversi Format & Peralatan Video Cepat)
Dirancang untuk content creator, video editor, dan tim produk yang sering membagikan rekaman layar.
- **Format Video Utama**: `MP4` (H.264 + AAC faststart), `MOV` (QuickTime), `WEBM` (VP9 + Opus), `MKV`, `AVI`, dan `FLV`.
- **Sub-ring `Video Tools` (`video-tools`)**:
  - **Video to GIF (`GIF`)**: Ubah klip video atau rekaman layar menjadi GIF animasi tajam dengan *palettegen* otomatis.
  - **Video Compress (`Compress`)**: Perkecil ukuran file video secara signifikan untuk dikirim lewat WhatsApp, Discord, atau email.
  - **Remove Audio (`Mute`)**: Buang jalur suara dari video secara instan tanpa re-encode video.
  - **Extract Audio (`Audio`)**: Ambil trek suara dari video langsung menjadi file audio.
  - **Downscale Resolusi (`1080p` & `720p`)**: Perkecil resolusi video 4K ke Full HD atau HD dengan menjaga rasio aspek.
- **Hybrid Engine**: Mendukung penuh `ffmpeg` dan memiliki fallback otomatis ke `/usr/bin/avconvert` bawaan macOS.

---

### D. Music Room (Konversi Audio & Ekstraksi Suara)
Dirancang untuk musisi, podcaster, sound designer, dan editor.
- **11 Format Audio Lengkap**:
  - **Main Ring (`music`)**: `MP3`, `WAV` (PCM 16-bit), `M4A` (AAC), `FLAC` (Lossless), `OGG` (Vorbis).
  - **Sub-ring `More Audio` (`music-more`)**: `AIFF`, `OPUS`, `AAC`, `CAF` (Apple CoreAudio), `ALAC` (Apple Lossless), `WMA`.
- **Video-to-Audio Langsung**: Seret file video apa pun (`MP4`, `MOV`, `MKV`, `WEBM`) ke dalam Music Room untuk langsung mengekstrak suaranya ke `MP3`, `WAV`, `FLAC`, `M4A`, dll.
- **Zero-Setup di macOS**: Menggunakan `/usr/bin/afconvert` & `/usr/bin/avconvert` bawaan macOS sehingga konversi `WAV`, `M4A`, `AAC`, `FLAC`, `AIFF`, `CAF`, dan `ALAC` langsung berjalan tanpa instalasi tambahan.

---

## 4. Tabel Referensi Lengkap: 117+ Ekstensi Input & 72+ Target Output

Tabel ini dapat ditampilkan di halaman **"Supported Formats"** atau bagian accordion pada website:

| Kategori Room | Ekstensi Input yang Didukung (117 Ekstensi) | Target Output & Smart Actions (72 Target) |
| :--- | :--- | :--- |
| **Photo Room** *(55 Input)* | `.png`, `.jpg`, `.jpeg`, `.jpe`, `.jfif`, `.webp`, `.avif`, `.heic`, `.heif`, `.hif`, `.gif`, `.bmp`, `.dib`, `.tif`, `.tiff`, `.ico`, `.cur`, `.icns`, `.tga`, `.exr`, `.hdr`, `.qoi`, `.ppm`, `.pgm`, `.pbm`, `.pnm`, `.pam`, `.dds`, `.ff`, `.svg`, `.psd`, `.jp2`, `.j2k`, `.dng`, `.cr2`, `.cr3`, `.nef`, `.nrw`, `.arw`, `.srf`, `.sr2`, `.raf`, `.orf`, `.rw2`, `.pef`, `.srw`, `.erf`, `.3fr`, `.fff`, `.iiq`, `.dcr`, `.kdc`, `.mrw`, `.raw`, `.rwl` | **Format (17)**: `PNG`, `JPG`, `WEBP`, `HEIC`, `AVIF`, `GIF`, `TIFF`, `BMP`, `ICO`, `ICNS`, `PDF`, `TGA`, `EXR`, `HDR`, `QOI`, `PPM`, `SVG`<br>**Tools (15)**: `No BG`, `Clean`, `1:1 Square`, `GIF+`, `1 MB`, `500 KB`, `200 KB`, `100 KB`, `Lossless`, `90°↻`, `90°↺`, `180°`, `Flip H`, `Flip V`, `B&W` |
| **Office & Data Room** *(35 Input)* | **Dokumen & Slide (21)**: `.pdf`, `.docx`, `.docm`, `.dotx`, `.dotm`, `.doc`, `.dot`, `.odt`, `.rtf`, `.md`, `.markdown`, `.txt`, `.text`, `.log`, `.html`, `.htm`, `.xhtml`, `.epub`, `.pptx`, `.ppt`, `.odp`<br>**Spreadsheet & Data (14)**: `.xlsx`, `.xlsm`, `.xltx`, `.xltm`, `.xls`, `.xlt`, `.ods`, `.csv`, `.tsv`, `.tab`, `.json`, `.yaml`, `.yml`, `.xml` | **Dokumen & E-Book (9)**: `PDF`, `DOCX`, `DOCX OCR`, `MD`, `TXT`, `HTML`, `RTF`, `EPUB`, `ODT`<br>**Spreadsheet & Data (8)**: `XLSX`, `CSV`, `TSV`, `JSON`, `YAML`, `XML`, `SQL`, `MD Table`<br>*(Plus render halaman dokumen/sheet ke seluruh 17 format gambar)* |
| **Video Room** *(14 Input)* | `.mp4`, `.m4v`, `.mov`, `.webm`, `.mkv`, `.avi`, `.flv`, `.wmv`, `.mpg`, `.mpeg`, `.ts`, `.mts`, `.m2ts`, `.3gp` | **Format (6)**: `MP4`, `MOV`, `WEBM`, `MKV`, `AVI`, `FLV`<br>**Video Tools (6)**: `GIF` (Clip), `Compress`, `Mute`, `Audio` (Extract), `1080p`, `720p` |
| **Music Room** *(13 Input + 14 Video)* | `.mp3`, `.wav`, `.wave`, `.m4a`, `.flac`, `.ogg`, `.oga`, `.aif`, `.aiff`, `.opus`, `.aac`, `.caf`, `.wma` *(serta seluruh ekstensi video)* | **Format Audio (11)**: `MP3`, `WAV`, `M4A`, `FLAC`, `OGG`, `AIFF`, `OPUS`, `AAC`, `CAF`, `ALAC`, `WMA` |
| **Option / Alt Ring** *(Global Modifier)* | Tahan tombol `⌥ Option` (macOS) atau `Alt` (Windows) saat drag file | **6 Quick Presets**: `Compress`, `25%`, `50%`, `75%`, `1920 px`, `1080 px` |

---

## 5. Perbandingan Flipfile vs Konverter Lain (Untuk Tabel Comparison di Website)

| Fitur | Konverter Cloud (CloudConvert, iLovePDF, dll.) | Aplikasi Konverter Desktop Lama | **Flipfile** |
| :--- | :--- | :--- | :--- |
| **Privasi File** | Diunggah ke server pihak ketiga | Lokal | **100% Lokal (Zero Upload)** |
| **Kecepatan Alur Kerja** | Buka browser → Upload → Pilih dropdown → Tunggu → Download | Buka jendela besar → Add files → Pilih output folder → Convert | **1 detik: Drag → Arahkan ke Ring → Lepas** |
| **Batas Ukuran & Kuota Harian** | Dibatasi (misal maks 5–25 file/hari untuk gratis) | Tidak terbatas | **Tanpa Batas (Unlimited Batch)** |
| **Cakupan Format** | Luas tapi butuh koneksi internet | Biasanya hanya 1 kategori (hanya video atau hanya foto) | **4 Kategori (Photo, Office/Data, Video, Music) — 117+ Input / 3.200+ Kombinasi** |
| **AI Remove Background** | Berbayar per kredit / langganan | Jarang ada | **Gratis & Offline (On-Device Neural Engine)** |
| **Konversi Data Developer (`CSV ⇄ JSON/YAML/SQL/XLSX`)** | Terpisah di banyak web tools | Tidak ada | **Tersedia langsung di Office Data Ring** |
| **Ukuran Aplikasi** | Butuh tab browser berat | 150 MB – 500 MB | **~5 MB (Native Rust + Tauri)** |

---

## 6. Fitur UX Eksklusif (Highlight Tambahan untuk Section "Why Users Love It")

1. **Ring Workshop ("Bengkel Ring") & Custom Workspaces (`Right-Click` / `Cmd+,`)**:
   Setiap orang punya kebutuhan format yang berbeda. Buka **Ring Workshop** untuk merakit sendiri isi Ring Anda secara *drag-and-drop*:
   - **Mode Direct Ring (1-Step)**: Gabungkan format lintas kategori favorit Anda (misal `WEBP`, `No BG`, `PDF`, `JSON`, `SQL`, `MP4`, `MP3`) dalam satu lingkaran utama tanpa perlu masuk ke sub-kategori.
   - **Mode Category Rooms**: Modifikasi slot di dalam masing-masing ruangan (`Photo`, `Office`, `Video`, `Music`) agar hanya menampilkan format yang sering Anda gunakan (mulai dari 2 hingga 10 slot per ring).
   - **Preset Workspace Siap Pakai & Custom**: Beralih cepat antara workspace *4 Rooms (Default)*, *Designer & Web*, *Developer & Data*, *Office & Docs*, *Creator & Media*, atau buat workspace baru sesuai proyek Anda.
2. **Clipboard Paste (`Cmd+V` / `Ctrl+V`)**:
   Habis ambil screenshot atau copy gambar dari browser/Figma? Cukup klik Flipfile lalu tekan `Cmd+V` / `Ctrl+V`, pilih format, dan hasilnya langsung tersimpan rapi di folder `Downloads`.
2. **One-Click Undo (`Undo Last Conversion`)**:
   Salah pilih format saat konversi 50 file sekaligus? Klik kanan ikon tray → **Undo Last Conversion** untuk memindahkan seluruh hasil konversi terakhir ke Trash dalam satu klik.
3. **Hold `⌥ Option` / `Alt` untuk Resize & Compress Instan**:
   Sambil menyeret foto, tahan tombol `⌥ Option` (macOS) atau `Alt` (Windows) untuk memilih ukuran (`25%`, `50%`, `75%`, `1080p`, `1920p`, `Compress`), lalu lepaskan pada format tujuan—atau lepaskan langsung di atas preset ukuran untuk mengecilkan file di format aslinya.
4. **Auto Update Notifier**:
   Mengecek rilis terbaru di GitHub secara ringan di latar belakang dan memberi tahu pengguna lewat menu tray saat versi baru tersedia.
5. **Native macOS Liquid Glass & Dark/Light Mode**:
   Menggunakan efek kaca buram native macOS (`NSVisualEffectView`) yang membesar mengikuti animasi piringan radial.

---

## 7. Struktur Halaman Website (Sitemap / Wireframe Copy)

Jika Anda membuat landing page (misalnya dengan Next.js, Astro, atau Tailwind), berikut urutan section yang paling efektif untuk konversi pengunjung menjadi pengunduh:

1. **Navbar**: Logo Flipfile · Features · 4 Rooms · Formats (117+) · Privacy · Download Button (`macOS` / `Windows`).
2. **Hero Section**:
   - Headline + Sub-headline.
   - Tombol CTA utama: **Download for macOS (Apple Silicon & Intel)** & **Download for Windows (64-bit)**.
   - Demo Video / Interactive Radial Ring Preview (pengunjung bisa mencoba mengarahkan kursor ke ring `Photo`, `Office`, `Video`, `Music` langsung di browser).
3. **Stats Banner**: `117+ Input Formats` · `3,200+ Conversions` · `100% On-Device Privacy` · `< 5 MB App Size`.
4. **Interactive "Explore the 4 Rooms" Section**:
   - Tab 1: **Photo Room** (Tampilkan fitur Remove BG, Target-Size 100KB, RAW/PSD support, Multi-size ICO/ICNS).
   - Tab 2: **Office & Data Room** (Tampilkan PDF ⇄ DOCX OCR, Markdown ⇄ EPUB, CSV/XLSX ⇄ JSON/SQL/YAML).
   - Tab 3: **Video Room** (Tampilkan Video to GIF, Compress, Mute, 1080p/720p).
   - Tab 4: **Music Room** (Tampilkan Audio conversion & Video-to-MP3/WAV/FLAC extraction).
5. **How It Works (3 Langkah Visual)**:
   - *Step 1*: Seret satu atau puluhan file ke orb Flipfile.
   - *Step 2*: Arahkan ke ruangan & format tujuan (tahan `⌥/Alt` jika ingin sekalian resize).
   - *Step 3*: Lepaskan! File hasil konversi langsung muncul di sebelah file asli Anda—klik orb untuk membukanya di Finder/Explorer.
6. **Comparison Table**: Flipfile vs Online Converters.
7. **Searchable Format Matrix**: Kotak pencarian interaktif ("Cari format, misal: `CR3`, `EPUB`, `SQL`, `HEIC`, `FLAC`...") yang menampilkan semua format yang didukung.
8. **FAQ & Footer**: Link GitHub Releases, Changelog, Lisensi MIT.

---

## 8. FAQ (Siap Pakai untuk Website)

**Q: Apakah file saya benar-benar tidak diunggah ke server?**
> **A:** Ya, 100%. Flipfile memproses seluruh gambar, dokumen PDF/Office, data, audio, dan video secara lokal di komputer Anda. Anda bahkan bisa mematikan Wi-Fi dan seluruh fitur konversi tetap berjalan normal.

**Q: Apakah hasil konversi akan menimpa (overwrite) file asli saya?**
> **A:** Tidak pernah. Flipfile selalu menyimpan file baru di sebelah file asli dengan nama yang bersih (misalnya `laporan_converted.pdf` atau `foto_nobg.png`), dan memiliki fitur **Undo Last Conversion** di menu bar jika Anda ingin membatalkan konversi terakhir.

**Q: Apakah saya perlu menginstal software tambahan?**
> **A:** Untuk sebagian besar fitur (semua format gambar, RAW, AI Remove BG, PDF, DOCX, XLSX, CSV, JSON, SQL, EPUB, serta audio/video utama di macOS), **tidak perlu menginstal apa pun**. Untuk codec video/audio lanjutan seperti `WEBM`, `MKV`, `OPUS`, atau `MP3`, Flipfile otomatis mendeteksi `ffmpeg` jika terinstal di sistem Anda (`brew install ffmpeg` di macOS atau `winget install ffmpeg` di Windows).

