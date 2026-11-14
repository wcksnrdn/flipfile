// Downloads the AI models into src-tauri/models/, where the platform's tauri.*.conf.json
// bundles them as resources:
// - AI upscaling (src-tauri/src/platform/upscale.rs):
//   - macOS: Core ML models, built by scripts/upscale-model/convert.py;
//   - Windows and Linux: the same models as ONNX, built by scripts/upscale-model/export_onnx.py,
//     with YuNet, the face detector.
//   They are published once as release assets of this repository, so builds don't need Python.
// - Transcribe (src-tauri/src/platform/whisper.rs): OpenAI's Whisper `small` model in
//   whisper.cpp's GGML format (5-bit weights), as published by the whisper.cpp project on
//   Hugging Face, unchanged; the same file on every system. With it Silero VAD (~1 MB),
//   which finds the speech in a recording first, also as published there.
//
// Supply-chain protection, as for PDFium (fetch-pdfium.mjs): the models end up inside
// every installer, so they are verified against SHA-256 hashes pinned below, twice:
//   1. the downloaded archive, before anything is extracted from it;
//   2. every extracted file, before it is used, and again on every later run, so a model
//      changed on disk is replaced instead of being bundled.
// Any mismatch stops the build. Nothing unverified is ever left in src-tauri/models/.
//
// FLIPFILE_ONNX=1 fetches the ONNX models on a Mac too, to test the Windows/Linux engine
// there. To publish new models, run the scripts, archive the folders and files
// (`tar -czf flipfile-models-N.tar.gz upscale.mlmodelc upscale-large.mlmodelc
// faces.mlmodelc faces-upscale.mlmodelc`; `tar -czf flipfile-models-onnx-N.tar.gz
// upscale.onnx upscale-large.onnx faces.onnx yunet.onnx faces-upscale.onnx`), attach both to a release `models-N` that is not marked as the
// latest release (the in-app updater reads the latest one), and record the hashes here.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const VERSION = "models-2";
const RELEASE = `https://github.com/wcksnrdn/flipfile/releases/download/${VERSION}`;

// Each set: its archive and pinned hash, the folders or files it holds (replaced as a
// whole), and the pinned hash of every file
const SETS = {
  coreml: {
    url: `${RELEASE}/flipfile-models-2.tar.gz`,
    archive: "42af598dcdbd6935e7f7ca262e8d2dd9b08fb8c750b27318186fadbd95b7028a",
    entries: ["upscale.mlmodelc", "upscale-large.mlmodelc", "faces.mlmodelc", "faces-upscale.mlmodelc"],
    files: {
      "upscale.mlmodelc/analytics/coremldata.bin": "939bd25cb6f733debaff2d96656db1d243ef104879967caf2b73be26b1560ff2",
      "upscale.mlmodelc/coremldata.bin": "9ab3dd8c107a184c44f096d44ddbc2093886a81618838e4d3916cd07b4c194aa",
      "upscale.mlmodelc/model.mil": "4fac2a38cbe1a49e35771d4b5a30978b23578c9fb00d27f93f6298c9703d0149",
      "upscale.mlmodelc/weights/weight.bin": "54850b2138ac1a8489448b93e8bc0b50fb24530d2999ff035c573f1cb7ff86b6",
      "upscale-large.mlmodelc/analytics/coremldata.bin": "06f5dd29fad3cb8ae4ef8fdbf471682a5364aaf92945b2eccbc735f86dda497d",
      "upscale-large.mlmodelc/coremldata.bin": "9fc55f1c3405bb060e9b68966dbef96b24131464a37ec3c3f58e2e540e710399",
      "upscale-large.mlmodelc/model.mil": "edc1dbe8ffa5e45dae0d5e16da7e9ed877f90c405af12112f20d0ac190b7db3c",
      "upscale-large.mlmodelc/weights/weight.bin": "b0771b2a3874414cc1bc409300447a3398f8096fd92515b27f348d1137c6db3e",
      "faces.mlmodelc/analytics/coremldata.bin": "e9e8208bf1d6e7df3838a2cfe7872a2b50a22241a28995fbdac8d526c1b0751b",
      "faces.mlmodelc/coremldata.bin": "22ae07ebcb7266421763cfcdeb456dcdcfb79f440d464c53e4e2a8232952cfaa",
      "faces.mlmodelc/model.mil": "e8eca3e9eff9c6f88fbc6fff9c029dbe536d18ac2040cebfa5435e4beea507d3",
      "faces.mlmodelc/weights/weight.bin": "9781e5c1d2f0d25c0ad1a83d77bc4b57260e97d32ec8dc30437c3fb392f62f52",
      "faces-upscale.mlmodelc/analytics/coremldata.bin": "00b579b74ef1392028e063e15079ad928d8f37662c52e2bc3332a96f4e52922a",
      "faces-upscale.mlmodelc/coremldata.bin": "775ef3470bff2a16e88bdc10426e475883d2b7458cb7504970ace1f2ff85599b",
      "faces-upscale.mlmodelc/model.mil": "91d3d2f06594905a3b52c38d980afbe387d06958f03be6a37a5fd212841dc7a9",
      "faces-upscale.mlmodelc/weights/weight.bin": "163b35b5ed44e505371b2e962be6d9daf648f9da7d078ce8b0910a3aa510f36d",
    },
  },
  whisper: {
    // One plain file, not an archive (no `entries`); ~190 MB
    url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
    archive: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
    file: "whisper-small.bin",
    size: 200 * 1024 * 1024,
    files: { "whisper-small.bin": "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb" },
  },
  vad: {
    url: "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin",
    archive: "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987",
    file: "whisper-vad.bin",
    size: 2 * 1024 * 1024,
    files: { "whisper-vad.bin": "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987" },
  },
  onnx: {
    url: `${RELEASE}/flipfile-models-onnx-2.tar.gz`,
    archive: "e74e913e3048851ff3d6e84a749902d2c2bf2d50d9e455708bf984385eb69cdb",
    entries: ["upscale.onnx", "upscale-large.onnx", "faces.onnx", "yunet.onnx", "faces-upscale.onnx"],
    files: {
      "upscale.onnx": "c88b3c063179e944ab58612cfa37526c9062d026103da8c09507f38b92ce2795",
      "upscale-large.onnx": "52580cc02fb8d84bbbeb7a1c6855d6ba958122b31671cc747265f29198d9b60d",
      "faces.onnx": "7954c397267b83a9972ca0baf223a729a47bcc055435c67056652db6bee134d6",
      "yunet.onnx": "ebafce4e3c118d6554634be5c27ab333b4c047a9a8c3faf1d7cf93101c22f0f0",
      "faces-upscale.onnx": "9b0da336ae79e207ecbfd4f3dad637296f15ea0b9b8e42547596e8b3a2af304f",
    },
  },
};

// Each upscaling archive is ~115 MB; anything far larger is not what we asked for.
const MAX_DOWNLOAD_BYTES = 160 * 1024 * 1024;

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const OUT_DIR = join(ROOT, "src-tauri", "models");

class ModelsError extends Error {}
// Throws, so that `finally` blocks still clean up temporary files.
const fail = (message) => {
  throw new ModelsError(message);
};

const sha256 = (data) => createHash("sha256").update(data).digest("hex");

// Windows' own tar: the GNU tar some shells put first on the PATH takes "C:\..." for a
// remote host
const TAR = process.platform === "win32" ? join(process.env.SystemRoot || "C:\\Windows", "System32", "tar.exe") : "tar";

/** The files of a set that are missing from `dir` or don't match their pinned hash */
const mismatches = (dir, set) =>
  Object.entries(set.files).filter(([path, hash]) => {
    const file = join(dir, path);
    return !existsSync(file) || sha256(readFileSync(file)) !== hash;
  });

async function fetchSet(name, set) {
  const bad = mismatches(OUT_DIR, set);
  if (bad.length === 0) return; // present and intact
  if (bad.length < Object.keys(set.files).length) {
    console.warn(`AI models: ${bad.map(([p]) => p).join(", ")} missing or changed; downloading them again.`);
  }

  const limit = set.size ?? MAX_DOWNLOAD_BYTES;
  console.log(set.file ? `Downloading ${set.file} (${name}, ~${Math.ceil(set.size / 1024 / 1024)} MB at most)…` : `Downloading the AI upscaling models (${VERSION}, ${name}, ~115 MB)…`);
  let res;
  try {
    res = await fetch(set.url, { redirect: "follow" });
  } catch (e) {
    fail(`could not download ${set.url}: ${e.message}`);
  }
  if (!res.ok) fail(`could not download ${set.url}: HTTP ${res.status}`);
  // Redirects must stay on HTTPS (GitHub serves release files from its CDN)
  if (!res.url.startsWith("https://")) fail(`download was redirected to a non-HTTPS URL: ${res.url}`);
  const declared = Number(res.headers.get("content-length") || 0);
  if (declared > limit) fail(`download is unexpectedly large (${declared} bytes)`);
  const archive = Buffer.from(await res.arrayBuffer());
  if (archive.length > limit) fail(`download is unexpectedly large (${archive.length} bytes)`);

  const archiveHash = sha256(archive);
  if (archiveHash !== set.archive) {
    fail(
      `the downloaded archive does NOT match the pinned hash. Refusing to use it.\n` +
        `  expected ${set.archive}\n  got      ${archiveHash}\n` +
        `This can mean the release was replaced or the download was tampered with.`,
    );
  }

  // A single model file: verified above, written through a temporary name
  if (set.file) {
    mkdirSync(OUT_DIR, { recursive: true });
    const partial = join(OUT_DIR, `${set.file}.partial`);
    writeFileSync(partial, archive);
    renameSync(partial, join(OUT_DIR, set.file));
    console.log(`${set.file} ready and verified (sha256 ${archiveHash.slice(0, 16)}…): ${OUT_DIR}`);
    return;
  }

  const work = mkdtempSync(join(tmpdir(), "flipfile-models-"));
  try {
    const archivePath = join(work, "models.tar.gz");
    writeFileSync(archivePath, archive);
    // Only the pinned files, into a private temp folder
    execFileSync(TAR, ["-xzf", archivePath, "-C", work, ...Object.keys(set.files)], { stdio: "inherit" });
    const wrong = mismatches(work, set);
    if (wrong.length > 0) {
      fail(`extracted files do NOT match their pinned hashes: ${wrong.map(([p]) => p).join(", ")}`);
    }

    // Each model is replaced only once everything is verified, through a temporary
    // name, so an interrupted run never leaves a partial model
    mkdirSync(OUT_DIR, { recursive: true });
    for (const entry of set.entries) {
      const partial = join(OUT_DIR, `${entry}.partial`);
      rmSync(partial, { recursive: true, force: true });
      cpSync(join(work, entry), partial, { recursive: true });
      rmSync(join(OUT_DIR, entry), { recursive: true, force: true });
      renameSync(partial, join(OUT_DIR, entry));
    }
    console.log(`AI models ready and verified (archive sha256 ${archiveHash.slice(0, 16)}…): ${OUT_DIR}`);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

async function main() {
  if (process.platform === "darwin") {
    await fetchSet("Core ML", SETS.coreml);
    if (process.env.FLIPFILE_ONNX) await fetchSet("ONNX", SETS.onnx);
  } else {
    await fetchSet("ONNX", SETS.onnx);
  }
  await fetchSet("GGML", SETS.whisper);
  await fetchSet("Silero VAD", SETS.vad);
}

try {
  await main();
} catch (e) {
  console.error(`\nAI models: ${e instanceof ModelsError ? e.message : e.stack || e}\n`);
  process.exitCode = 1;
}
