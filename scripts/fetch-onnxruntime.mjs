// Downloads ONNX Runtime, the engine that runs the AI upscaling models on Windows and
// Linux (src-tauri/src/platform/onnx.rs), into src-tauri/onnxruntime/, where
// tauri.windows.conf.json / tauri.linux.conf.json bundle it as a resource. macOS runs the
// models with Core ML instead and needs none of this.
//
// What is fetched, all official Microsoft releases of ONNX Runtime 1.24.4 (the last one
// with DirectML; MIT licence):
// - Windows: onnxruntime.dll with DirectML, which runs the models on any DirectX 12 GPU
//   (NuGet Microsoft.ML.OnnxRuntime.DirectML), and DirectML.dll itself (NuGet
//   Microsoft.AI.DirectML, Microsoft's redistributable licence for Windows apps).
// - Linux: libonnxruntime.so (CPU), from the GitHub release.
// - macOS (only with FLIPFILE_ONNX=1): libonnxruntime.dylib, to test the Windows/Linux
//   engine on a Mac (`cargo test` then runs it too).
//
// Supply-chain protection, as for PDFium (fetch-pdfium.mjs): these libraries end up
// inside every installer, so they are verified against SHA-256 hashes pinned below,
// twice: each downloaded archive before anything is extracted from it, and every
// extracted file before it is used and again on every later run. Any mismatch stops
// the build. Nothing unverified is ever left in src-tauri/onnxruntime/.
//
// Override the platform when cross-building, e.g. ONNXRUNTIME_PLATFORM=linux-arm64.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const VERSION = "1.24.4";
const GITHUB = `https://github.com/microsoft/onnxruntime/releases/download/v${VERSION}`;
const NUGET = "https://api.nuget.org/v3-flatcontainer";

// Per platform: the archives to download, and from each, the files to keep
// (path in the archive -> [name in src-tauri/onnxruntime, pinned hash]).
const PLATFORMS = {
  "win-x64": [
    {
      url: `${NUGET}/microsoft.ml.onnxruntime.directml/${VERSION}/microsoft.ml.onnxruntime.directml.${VERSION}.nupkg`,
      archive: "57e9f11b73437bef7a309496135d4c1f96b1a8e9ddba60013fa27bfc1d788681",
      files: {
        "runtimes/win-x64/native/onnxruntime.dll": ["onnxruntime.dll", "e7eedec6a6f26dc39dc948276a75ef6d2bee3fff944d874ceed0bbd3b97bff40"],
        LICENSE: ["LICENSE-onnxruntime.txt", "c250d6278f0b47a6439fb7592b08b58a55eb9f535aa49a1db63211c3f982b674"],
        "ThirdPartyNotices.txt": ["ThirdPartyNotices-onnxruntime.txt", "fb0af774b4d7cffc5b9d046f2aaeade2f37df2f80abf8033c95dfffcc77a8866"],
      },
    },
    {
      // ~200 MB: every architecture and debug build of DirectML; only one DLL is kept
      url: `${NUGET}/microsoft.ai.directml/1.15.4/microsoft.ai.directml.1.15.4.nupkg`,
      archive: "4e7cb7ddce8cf837a7a75dc029209b520ca0101470fcdf275c1f49736a3615b9",
      files: {
        "bin/x64-win/DirectML.dll": ["DirectML.dll", "9c9e6d822561c6c41b90e6994b3e8857cf1d66dbfb1e0c4c799c7c89b4e92da1"],
        "LICENSE.txt": ["LICENSE-DirectML.txt", "a05138e3a085ff60a44881eedfa58dccb03ecc1d7b1f6ae888418e8c2fec4b8d"],
      },
    },
  ],
  "linux-x64": [
    {
      url: `${GITHUB}/onnxruntime-linux-x64-${VERSION}.tgz`,
      archive: "3a211fbea252c1e66290658f1b735b772056149f28321e71c308942cdb54b747",
      files: {
        [`onnxruntime-linux-x64-${VERSION}/lib/libonnxruntime.so.${VERSION}`]: ["libonnxruntime.so", "d132535d051344ff5c64c9c200004150559049a81ed330eb4422c1962fb6b7e4"],
        [`onnxruntime-linux-x64-${VERSION}/LICENSE`]: ["LICENSE-onnxruntime.txt", "2f07c72751aed99790b8a4869cf2311df85a860b22ded05fa22803587a48922c"],
        [`onnxruntime-linux-x64-${VERSION}/ThirdPartyNotices.txt`]: ["ThirdPartyNotices-onnxruntime.txt", "0e07b95f3a8d6230037707c5c4a2b554d12c4cb67369669ac255635528ffcee2"],
      },
    },
  ],
  "linux-arm64": [
    {
      url: `${GITHUB}/onnxruntime-linux-aarch64-${VERSION}.tgz`,
      archive: "866109a9248d057671a039b9d725be4bd86888e3754140e6701ec621be9d4d7e",
      files: {
        [`onnxruntime-linux-aarch64-${VERSION}/lib/libonnxruntime.so.${VERSION}`]: ["libonnxruntime.so", "52b2a0e75e79468404284fec38ad5ee1a7a996232274f5e1b84f3e793fb07554"],
        [`onnxruntime-linux-aarch64-${VERSION}/LICENSE`]: ["LICENSE-onnxruntime.txt", "2f07c72751aed99790b8a4869cf2311df85a860b22ded05fa22803587a48922c"],
        [`onnxruntime-linux-aarch64-${VERSION}/ThirdPartyNotices.txt`]: ["ThirdPartyNotices-onnxruntime.txt", "0e07b95f3a8d6230037707c5c4a2b554d12c4cb67369669ac255635528ffcee2"],
      },
    },
  ],
  "mac-arm64": [
    {
      url: `${GITHUB}/onnxruntime-osx-arm64-${VERSION}.tgz`,
      archive: "93787795f47e1eee369182e43ed51b9e5da0878ab0346aecf4258979b8bba989",
      files: {
        [`onnxruntime-osx-arm64-${VERSION}/lib/libonnxruntime.${VERSION}.dylib`]: ["libonnxruntime.dylib", "872533f130f1839a5bc01788ddb4f75c83a189763441ba1178788ed965449289"],
      },
    },
  ],
};

// The largest archive (DirectML) is ~200 MB; anything far larger is not what we asked for.
const MAX_DOWNLOAD_BYTES = 256 * 1024 * 1024;

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const OUT_DIR = join(ROOT, "src-tauri", "onnxruntime");

class RuntimeError extends Error {}
// Throws, so that `finally` blocks still clean up temporary files.
const fail = (message) => {
  throw new RuntimeError(message);
};

const sha256 = (data) => createHash("sha256").update(data).digest("hex");

const detectPlatform = () => {
  const arch = process.arch === "arm64" ? "arm64" : "x64";
  if (process.platform === "darwin") return `mac-${arch}`;
  if (process.platform === "win32") return `win-${arch}`;
  return `linux-${arch}`;
};

// Windows' own tar and macOS' (bsdtar) also read the NuGet packages, which are zip files;
// GNU tar (Linux, and first on the PATH in some Windows shells) can't, so Linux uses unzip
const TAR = process.platform === "win32" ? join(process.env.SystemRoot || "C:\\Windows", "System32", "tar.exe") : "tar";
const extract = (archive, into, files, zip) =>
  zip && process.platform === "linux"
    ? execFileSync("unzip", ["-q", archive, ...files, "-d", into], { stdio: "inherit" })
    : execFileSync(TAR, ["-xf", archive, "-C", into, ...files], { stdio: "inherit" });

/** The kept files of `parts` that are missing from `dir` or don't match their pinned hash */
const mismatches = (dir, parts) =>
  parts.flatMap((part) =>
    Object.values(part.files).filter(([name, hash]) => {
      const file = join(dir, name);
      return !existsSync(file) || sha256(readFileSync(file)) !== hash;
    }),
  );

async function download(url) {
  let res;
  try {
    res = await fetch(url, { redirect: "follow" });
  } catch (e) {
    fail(`could not download ${url}: ${e.message}`);
  }
  if (!res.ok) fail(`could not download ${url}: HTTP ${res.status}`);
  // Redirects must stay on HTTPS (GitHub and NuGet serve files from their CDNs)
  if (!res.url.startsWith("https://")) fail(`download was redirected to a non-HTTPS URL: ${res.url}`);
  const declared = Number(res.headers.get("content-length") || 0);
  if (declared > MAX_DOWNLOAD_BYTES) fail(`download is unexpectedly large (${declared} bytes)`);
  const data = Buffer.from(await res.arrayBuffer());
  if (data.length > MAX_DOWNLOAD_BYTES) fail(`download is unexpectedly large (${data.length} bytes)`);
  return data;
}

async function main() {
  const target = process.env.ONNXRUNTIME_PLATFORM || detectPlatform();
  if (target.startsWith("mac-") && !process.env.FLIPFILE_ONNX && !process.env.ONNXRUNTIME_PLATFORM) return;
  const parts = PLATFORMS[target];
  if (!parts) {
    // Only known platforms: the value picks the download URLs
    fail(`unknown platform "${target}". Use one of: ${Object.keys(PLATFORMS).join(", ")}`);
  }

  const marker = join(OUT_DIR, ".version");
  const current = existsSync(marker) && readFileSync(marker, "utf8") === `${VERSION} ${target}`;
  if (current && mismatches(OUT_DIR, parts).length === 0) return; // present and intact
  if (current) console.warn("ONNX Runtime: files missing or changed; downloading them again.");

  const work = mkdtempSync(join(tmpdir(), "flipfile-onnxruntime-"));
  try {
    for (const [i, part] of parts.entries()) {
      console.log(`Downloading ONNX Runtime ${VERSION} (${target}, ${i + 1}/${parts.length})…`);
      const archive = await download(part.url);
      const hash = sha256(archive);
      if (hash !== part.archive) {
        fail(
          `${part.url} does NOT match its pinned hash. Refusing to use it.\n` +
            `  expected ${part.archive}\n  got      ${hash}\n` +
            `This can mean the release was replaced or the download was tampered with.`,
        );
      }
      const archivePath = join(work, `part${i}`);
      const into = join(work, `files${i}`);
      writeFileSync(archivePath, archive);
      mkdirSync(into);
      // Only the pinned files, into a private temp folder
      extract(archivePath, into, Object.keys(part.files), part.url.endsWith(".nupkg"));
      for (const [path, [name, pinned]] of Object.entries(part.files)) {
        const got = sha256(readFileSync(join(into, path)));
        if (got !== pinned) fail(`${path} from ${part.url} does NOT match its pinned hash`);
        copyFileSync(join(into, path), join(work, name));
      }
    }

    // Replaced only once everything is verified
    rmSync(OUT_DIR, { recursive: true, force: true });
    mkdirSync(OUT_DIR, { recursive: true });
    for (const part of parts) {
      for (const [name] of Object.values(part.files)) copyFileSync(join(work, name), join(OUT_DIR, name));
    }
    if (mismatches(OUT_DIR, parts).length > 0) fail("files changed while being copied");
    writeFileSync(marker, `${VERSION} ${target}`);
    console.log(`ONNX Runtime ready and verified: ${OUT_DIR}`);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

try {
  await main();
} catch (e) {
  console.error(`\nONNX Runtime: ${e instanceof RuntimeError ? e.message : e.stack || e}\n`);
  process.exitCode = 1;
}
