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
// - Upscale Photo and Upscale Text (models-3): the detailed photo upscaler (Nomos2), the
//   document upscaler (RealESRNet), the JPEG cleaner (FBCNN) and the colourer (DDColor),
//   as Core ML and ONNX models like the first ones; and for Upscale Text on every system
//   the open fonts it types words in (models/fonts) and the Indonesian and English word
//   list Windows and Linux check words against (models/words.txt).
//
// FLIPFILE_ONNX=1 fetches the ONNX models on a Mac too, to test the Windows/Linux engine
// there. To publish new models, run the scripts, archive the folders and files with
// COPYFILE_DISABLE=1 (so macOS adds no ._ files), e.g. `tar -czf flipfile-models-N.tar.gz
// upscale-photo.mlmodelc ...` and `tar -czf flipfile-models-onnx-N.tar.gz upscale-photo.onnx
// ...`, attach them to a release `models-N` that is not marked as the latest release (the
// in-app updater reads the latest one), and record the hashes here. Archives already
// published stay as they are: a new release only adds sets.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const RELEASES = "https://github.com/wcksnrdn/flipfile/releases/download";
const RELEASE = `${RELEASES}/models-2`;
const RELEASE_3 = `${RELEASES}/models-3`;

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
  coreml3: {
    url: `${RELEASE_3}/flipfile-models-3.tar.gz`,
    archive: "8a36f889ddc861b0711424972f0c4609dc046c758930f395f0ece093589c2350",
    size: 300 * 1024 * 1024,
    entries: ["upscale-photo.mlmodelc", "upscale-text.mlmodelc", "dejpeg.mlmodelc", "colorize.mlmodelc"],
    files: {
      "upscale-photo.mlmodelc/analytics/coremldata.bin": "635d78ba72f5a982753abf759cefaedde1c5a84f694fc2b49f351e764d0149c4",
      "upscale-photo.mlmodelc/coremldata.bin": "2903962b9f008248d38b21d9bf53b8f01583f493dac40150b2bb729126879cf2",
      "upscale-photo.mlmodelc/model.mil": "51e3330e20569a33268f7b815912f6053b84b446d48e2a1e3d16e48705bfe23f",
      "upscale-photo.mlmodelc/weights/weight.bin": "633b492030a151902952eb8d6e2ef606708235e227a6a258348e5a648652e779",
      "upscale-text.mlmodelc/analytics/coremldata.bin": "ceb5b061ec036c134a94b668e66d22190c1851fbe3f4e6318220b3613f963637",
      "upscale-text.mlmodelc/coremldata.bin": "60d3d67c292df57252650820a3df15c727b9f87593a7e31f4d2875a572af9f09",
      "upscale-text.mlmodelc/model.mil": "a2dc13f1c563c76e8525046f337fd580ef3a5901ce4ec8d28fd36ddde6caf487",
      "upscale-text.mlmodelc/weights/weight.bin": "496a870deac4a7faaeb72677c3312a5e02d9be02e0f1da009f662f3d22acffd1",
      "dejpeg.mlmodelc/analytics/coremldata.bin": "7734829bec25e6f35c34aab053082ec453ba3b4acaa58582cd82440beb84ade3",
      "dejpeg.mlmodelc/coremldata.bin": "db4c88a31942759e4dd9fed96e5216305858b3943c9e13b46b31f3618b6b7d03",
      "dejpeg.mlmodelc/model.mil": "7bc8dece75345c930026b8ab43cd7c8a8ba048b727c5da328b5bbad631893b6b",
      "dejpeg.mlmodelc/weights/weight.bin": "a8a12b8e0ab57f0c865ea2ed5113b242782c375b4ff28899999187f3dbe5ef33",
      "colorize.mlmodelc/analytics/coremldata.bin": "675c0e12399471d9ccd0adcb1674bc07d0ba9c7d38f92d22a93ba37d6f68c04e",
      "colorize.mlmodelc/coremldata.bin": "c714cd4241ebe236fb7c3a4192e2ff918bd8afff369782a595c7f2880e386c80",
      "colorize.mlmodelc/model.mil": "3b0f093da52325efe9fdf0ee4500aa523e9bc9a117d4d280e673800adfb22e1f",
      "colorize.mlmodelc/weights/weight.bin": "e4c0a38e521c8bd5b502ea390417e9cccc42dcbeb8e7d4b3c7586283ab67585b",
    },
  },
  onnx3: {
    url: `${RELEASE_3}/flipfile-models-onnx-3.tar.gz`,
    archive: "a86cd855db08202ba2bbbcbc559d7b2339a12c31ca9236c03a7fa781a740b07c",
    size: 330 * 1024 * 1024,
    entries: ["upscale-photo.onnx", "upscale-text.onnx", "dejpeg.onnx", "colorize.onnx"],
    files: {
      "upscale-photo.onnx": "6e06fe2b455d31f48073cef7b13c61d90a0ff59d293e685030da8245abd653ec",
      "upscale-text.onnx": "ecea9700b2652e40fb8b3ca862256f96ca23f3f01170da097e99db51db788ed7",
      "dejpeg.onnx": "94a69c887f1835f671383e213173ab2c28e067af7f8229e09ff162edb91e0fd8",
      "colorize.onnx": "731a75086129b6863d3a12394be89254687bb1da24050fbf7d4857690b32db52",
    },
  },
  text3: {
    url: `${RELEASE_3}/flipfile-text-3.tar.gz`,
    archive: "36c1d9e790daa556ebf518fab45a46638986e3ec9e8b8e7c3706fab0563dfdc3",
    size: 8 * 1024 * 1024,
    entries: ["fonts", "words.txt"],
    files: {
      "words.txt": "7e2c95e3ce133699fb23226cff35a5d40f05f992b60a7c10da0784077c18974f",
      "fonts/Caladea-Bold.ttf": "ae3cb2dcbc925809dd29d2a44e9802211cab66be541bacbfc9c08c74b27c3742",
      "fonts/Caladea-Regular.ttf": "f1e899278b7b4491aba5b6a8253c4b04c050cc59b21865be5c37559a775153cd",
      "fonts/Carlito-Bold.ttf": "bb5d20f79b82599ec72983597437373a80f2d2085fa91fc144fd74e876a594db",
      "fonts/Carlito-Regular.ttf": "f6418f708baede9789daef5d458c0f53d2a888af9820e8062934e504fedc6595",
      "fonts/DejaVuSans-Bold.ttf": "e6476c1b80502924294eed40894c5b18e06c181444ca953e5334262df9c27724",
      "fonts/DejaVuSans-Regular.ttf": "7da195a74c55bef988d0d48f9508bd5d849425c1770dba5d7bfc6ce9ed848954",
      "fonts/Gelasio-Bold.ttf": "c01bd0058d09af2394f666daf74560a48c26ed231f0a87d5144b9ae8395c1f42",
      "fonts/Gelasio-Regular.ttf": "195e28eef8f2e950bb3783f0eb40a27a0569941d815b4d16e58bad6f922a80e2",
      "fonts/Inter-Bold.ttf": "288316099b1e0a47a4716d159098005eef7c0066921f34e3200393dbdb01947f",
      "fonts/Inter-Regular.ttf": "40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82",
      "fonts/LiberationMono-Regular.ttf": "f2b83c763e8afd21709333370bed4774337fae82267937e2b5aea7e2fbd922c1",
      "fonts/LiberationSans-Bold.ttf": "788abee4c806d660e8aee46689dd8540cd4bb98da03dcc9d171ce3efd99a9173",
      "fonts/LiberationSans-Regular.ttf": "76d04c18ea243f426b7de1f3ad208e927008f961dc5945e5aad352d0dfde8ee8",
      "fonts/LiberationSerif-Bold.ttf": "d754ba427cfe0bca54ae052384baa8f842da5bd6550ad4da024ac441e7a7d5ce",
      "fonts/LiberationSerif-Regular.ttf": "058ea80864aef09a23f45cbec2bb5400bc3dfbdea01c3f10538a21fcb497fb74",
      "fonts/NotoSans-Bold.ttf": "d4c1ad15902ea24831a4ae5b1f3fece4b282ce3cd655e39d3f8ec2767206972e",
      "fonts/NotoSans-Regular.ttf": "3d6f377b52cecb32dbb4c441265b325a7d38c6d2bfc3cd400f81cc3475c91bee",
      "fonts/OpenSans-Bold.ttf": "4129aa7d0d626cb6eb5772918556303ef967fd52b528f3d55f629f62852122ed",
      "fonts/OpenSans-Regular.ttf": "869d9e65c730b937f02b311c32635623c3d67cf801a9d2ac558f7704e4988c0d",
      "fonts/Roboto-Bold.ttf": "15216b8c5fd9ccfe4ecfc316eb8038ecb556814cea2f1eabda4c9d41e434becb",
      "fonts/Roboto-Regular.ttf": "39548fd9d6250717a5fcfe00b1b0ade234c1f5be8a40818c37e995c13c844df1",
      "fonts/LICENSE-Caladea.txt": "ccdab61d371d8c8683a128a92cd7d498dbdb1d37689f7cb21f1bf6b16658d213",
      "fonts/LICENSE-Carlito.txt": "58402f82a7c332a700294988fe7554fbb0a63a8d27ccc1ee3bbc640311990a00",
      "fonts/LICENSE-DejaVu.txt": "7a083b136e64d064794c3419751e5c7dd10d2f64c108fe5ba161eae5e5958a93",
      "fonts/LICENSE-Gelasio.txt": "b393cb01867c919b44381512120dc3e4c954c7b47e2035c405f3a324799a4d29",
      "fonts/LICENSE-Inter.txt": "262481e844521b326f5ecd053e59b98c8b2da78c8ee1bdbb6e8174305e54935a",
      "fonts/LICENSE-Liberation.txt": "93fed46019c38bbe566b479d22148e2e8a1e85ada614accb0211c37b2c61c19b",
      "fonts/LICENSE-NotoSans.txt": "cee9892f9f0cc8fe882c9e9537ee6a89621d86ee7ceaf70b02e2b2b1c25c061a",
      "fonts/LICENSE-OpenSans.txt": "fbbbcfef55318de350562559b671360de6d597112ecc5c73881b05092db89602",
      "fonts/LICENSE-Roboto.txt": "061402327a96aadb0bfb694a960ed289ecd38d383e396243831ab81feb109c41",
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

// Each models-2 upscaling archive is ~115 MB; anything far larger is not what we asked
// for. Larger sets give their own limit (`size`).
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
  console.log(`Downloading ${set.file ?? set.url.split("/").pop()} (${name}, ~${Math.ceil(limit / 1024 / 1024)} MB at most)…`);
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
    await fetchSet("Core ML, Upscale Photo and Text", SETS.coreml3);
    if (process.env.FLIPFILE_ONNX) {
      await fetchSet("ONNX", SETS.onnx);
      await fetchSet("ONNX, Upscale Photo and Text", SETS.onnx3);
    }
  } else {
    await fetchSet("ONNX", SETS.onnx);
    await fetchSet("ONNX, Upscale Photo and Text", SETS.onnx3);
  }
  await fetchSet("fonts and words for Upscale Text", SETS.text3);
  await fetchSet("GGML", SETS.whisper);
  await fetchSet("Silero VAD", SETS.vad);
}

try {
  await main();
} catch (e) {
  console.error(`\nAI models: ${e instanceof ModelsError ? e.message : e.stack || e}\n`);
  process.exitCode = 1;
}
