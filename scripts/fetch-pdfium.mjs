// Downloads the PDFium shared library for the current platform into src-tauri/pdfium/,
// where tauri.conf.json bundles it as a resource (used for PDF -> image conversion).
//
// Supply-chain protection: the library ends up inside every installer, so it is
// verified against SHA-256 hashes pinned below, twice:
//   1. the downloaded archive, before anything is extracted from it;
//   2. the extracted library, before it is used, and again on every later run, so a
//      library tampered with on disk is replaced instead of being bundled.
// Any mismatch stops the build. Nothing unverified is ever left in src-tauri/pdfium/.
//
// The hashes were recorded from the official release and cross-checked against the
// GitHub build attestations of bblanchon/pdfium-binaries (signed by its
// .github/workflows/build-all.yml workflow). To update PDFium, change VERSION, then for
// every platform download the archive and run
//   gh attestation verify pdfium-<platform>.tgz --repo bblanchon/pdfium-binaries
// and only then record the new archive and library hashes here. VERSION must match the
// PDFium API of the pdfium-render crate (its `pdfium_latest` feature).
//
// Override the platform when cross-building, e.g. PDFIUM_PLATFORM=mac-x64.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const VERSION = "chromium/7881"; // matches pdfium-render 0.9.4's `pdfium_latest`

// Per platform: path of the library inside the archive, and the pinned hashes.
const PLATFORMS = {
  "mac-arm64": {
    library: "lib/libpdfium.dylib",
    archive: "52e94ca5aa8847934330daf3f8150c190682c5ca93831468794f8b90d4392e40",
    sha256: "1bc45b15466b34cef96641ce25c77a876e70010c6b114f909dda2f5325fc5bd7",
  },
  "mac-x64": {
    library: "lib/libpdfium.dylib",
    archive: "6dedf83990e0e3d6b7c93c9e7589c5a126b0ae14b7464d76120cff7a26afb18b",
    sha256: "4eaad6c3e8d786cf6f66a45d7d014edf5c65f372f98c3070e66595ebb50e43d9",
  },
  "win-x64": {
    library: "bin/pdfium.dll",
    archive: "73cc0de638ac2095e7445bf56a38200a5b7c7ca0e9f4ba144598f2457377ac08",
    sha256: "79d4676b656cfb1abcea88f9ade3b4b0826c5200382db5f4ec72a636c598c118",
  },
  "win-arm64": {
    library: "bin/pdfium.dll",
    archive: "d3035d4d2cacac6ecd1a2ece197a3d702a1b2a58466276b9f870b8cb278a9d84",
    sha256: "267a6f08a9c854d9949754a53b7630f23c0b67de5c7ca273b6abf178b49158c2",
  },
  "linux-x64": {
    library: "lib/libpdfium.so",
    archive: "1470e21b8b4a3b4ad7f85684e2da11d94f3b69a86d81dee11b9b6709d927ac1d",
    sha256: "f728930966f503652b92acc89b9374a2eeca00ce42e26dccd3e4b5c5161b2d64",
  },
  "linux-arm64": {
    library: "lib/libpdfium.so",
    archive: "ee7f7b7d5468958336a818c1cd580bdd20972846b7377b13f9a923d92d1d4674",
    sha256: "6252fce3da45e7f0dc5b27f4d4e1a1456ca3f7734cdb04f927967df772127478",
  },
};

// The archives are ~4 MB; anything far larger is not what we asked for.
const MAX_DOWNLOAD_BYTES = 64 * 1024 * 1024;

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const OUT_DIR = join(ROOT, "src-tauri", "pdfium");

class PdfiumError extends Error {}
// Throws, so that `finally` blocks still clean up temporary files.
const fail = (message) => {
  throw new PdfiumError(message);
};

const sha256 = (data) => createHash("sha256").update(data).digest("hex");

const detectPlatform = () => {
  const arch = process.arch === "arm64" ? "arm64" : "x64";
  if (process.platform === "darwin") return `mac-${arch}`;
  if (process.platform === "win32") return `win-${arch}`;
  return `linux-${arch}`;
};

async function main() {
  const target = process.env.PDFIUM_PLATFORM || detectPlatform();
  const spec = PLATFORMS[target];
  if (!spec) {
    // Only known platforms: the value ends up in a download URL.
    fail(
      `unknown platform "${target}". Use one of: ${Object.keys(PLATFORMS).join(", ")}`,
    );
  }

  const libName = spec.library.split("/").pop();
  const dest = join(OUT_DIR, libName);
  const marker = join(OUT_DIR, ".version");

  // Already present and intact? (The hash check also catches a modified library.)
  if (
    existsSync(dest) &&
    existsSync(marker) &&
    readFileSync(marker, "utf8") === `${VERSION} ${target}`
  ) {
    if (sha256(readFileSync(dest)) === spec.sha256) return; // present and intact
    console.warn(
      `PDFium: ${dest} does not match its pinned hash; downloading it again.`,
    );
  }

  const url = `https://github.com/bblanchon/pdfium-binaries/releases/download/${encodeURIComponent(VERSION)}/pdfium-${target}.tgz`;
  console.log(`Downloading PDFium (${VERSION}, ${target})…`);

  let res;
  try {
    res = await fetch(url, { redirect: "follow" });
  } catch (e) {
    fail(`could not download ${url}: ${e.message}`);
  }
  if (!res.ok) fail(`could not download ${url}: HTTP ${res.status}`);
  // Redirects must stay on HTTPS (GitHub serves release files from its CDN)
  if (!res.url.startsWith("https://"))
    fail(`download was redirected to a non-HTTPS URL: ${res.url}`);
  const declared = Number(res.headers.get("content-length") || 0);
  if (declared > MAX_DOWNLOAD_BYTES)
    fail(`download is unexpectedly large (${declared} bytes)`);
  const archive = Buffer.from(await res.arrayBuffer());
  if (archive.length > MAX_DOWNLOAD_BYTES)
    fail(`download is unexpectedly large (${archive.length} bytes)`);

  const archiveHash = sha256(archive);
  if (archiveHash !== spec.archive) {
    fail(
      `the downloaded archive does NOT match the pinned hash. Refusing to use it.\n` +
        `  expected ${spec.archive}\n  got      ${archiveHash}\n` +
        `This can mean the release was replaced or the download was tampered with.`,
    );
  }

  const work = mkdtempSync(join(tmpdir(), "pdfium-"));
  try {
    const archivePath = join(work, "pdfium.tgz");
    writeFileSync(archivePath, archive);
    // Extract only the one member we need, into a private temp folder.
    // `tar` ships with macOS, Linux and Windows 10+ (bsdtar).
    execFileSync("tar", ["-xzf", archivePath, "-C", work, spec.library], {
      stdio: "inherit",
    });

    const extracted = join(work, spec.library);
    const libHash = sha256(readFileSync(extracted));
    if (libHash !== spec.sha256) {
      fail(
        `the extracted library does NOT match the pinned hash.\n  expected ${spec.sha256}\n  got      ${libHash}`,
      );
    }

    // Replace the folder contents only once everything is verified; write the library
    // under a temporary name first so an interrupted run never leaves a partial file.
    rmSync(OUT_DIR, { recursive: true, force: true });
    mkdirSync(OUT_DIR, { recursive: true });
    const partial = `${dest}.partial`;
    copyFileSync(extracted, partial);
    renameSync(partial, dest);
    writeFileSync(marker, `${VERSION} ${target}`);
    console.log(
      `PDFium ready and verified (sha256 ${libHash.slice(0, 16)}…): ${dest}`,
    );
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

try {
  await main();
} catch (e) {
  console.error(
    `\nPDFium: ${e instanceof PdfiumError ? e.message : e.stack || e}\n`,
  );
  process.exitCode = 1;
}
