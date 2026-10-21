// Downloads the PDFium shared library for the current platform into src-tauri/pdfium/,
// where tauri.conf.json bundles it as a resource (used for PDF -> image conversion).
//
// The version is pinned to the PDFium API that pdfium-render was built against.
// Override the platform when cross-building, e.g. PDFIUM_PLATFORM=mac-x64.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, copyFileSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const VERSION = "chromium/7881"; // matches pdfium-render 0.9.4's `pdfium_latest`

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const OUT_DIR = join(ROOT, "src-tauri", "pdfium");

const platform = () => {
  if (process.env.PDFIUM_PLATFORM) return process.env.PDFIUM_PLATFORM;
  const arch = process.arch === "arm64" ? "arm64" : "x64";
  if (process.platform === "darwin") return `mac-${arch}`;
  if (process.platform === "win32") return `win-${arch}`;
  return `linux-${arch}`;
};

// Where the library sits inside the archive, and its file name.
const library = (p) =>
  p.startsWith("mac") ? "lib/libpdfium.dylib" : p.startsWith("win") ? "bin/pdfium.dll" : "lib/libpdfium.so";

const target = platform();
const libPath = library(target);
const dest = join(OUT_DIR, libPath.split("/").pop());
const marker = join(OUT_DIR, ".version");

if (existsSync(dest) && existsSync(marker) && readFileSync(marker, "utf8") === `${VERSION} ${target}`) {
  process.exit(0); // already present
}

const url = `https://github.com/bblanchon/pdfium-binaries/releases/download/${encodeURIComponent(VERSION)}/pdfium-${target}.tgz`;
console.log(`Downloading PDFium (${VERSION}, ${target})…`);
const res = await fetch(url);
if (!res.ok) {
  console.error(`Could not download ${url}: HTTP ${res.status}`);
  process.exit(1);
}

const work = mkdtempSync(join(tmpdir(), "pdfium-"));
try {
  const archive = join(work, "pdfium.tgz");
  writeFileSync(archive, Buffer.from(await res.arrayBuffer()));
  // `tar` ships with macOS, Linux and Windows 10+ (bsdtar)
  execFileSync("tar", ["-xzf", archive, "-C", work, libPath]);
  rmSync(OUT_DIR, { recursive: true, force: true });
  mkdirSync(OUT_DIR, { recursive: true });
  copyFileSync(join(work, libPath), dest);
  writeFileSync(marker, `${VERSION} ${target}`);
  console.log(`PDFium ready: ${dest}`);
} finally {
  rmSync(work, { recursive: true, force: true });
}
