// The AI feature packs: the models flipfile downloads on demand instead of bundling
// (src-tauri/src/packs). Each pack is one feature of the ring; its files are listed in
// src-tauri/packs/catalog.json with their SHA-256 and size, which the app checks every
// download against. A file is published once, as a release asset of the `packs` release
// named by its SHA-256, so a new version of a pack only uploads the files that changed
// and the app only downloads those.
//
//   node scripts/packs.mjs catalog   rewrites catalog.json from src-tauri/models, which
//                                    `npm run fetch-models` fills and verifies (on a Mac,
//                                    with FLIPFILE_ONNX=1 for the ONNX models too)
//   node scripts/packs.mjs check     fails when catalog.json doesn't match those models
//   node scripts/packs.mjs publish   uploads the files the `packs` release doesn't have
//                                    yet (with gh), then catalog.json as catalog-<FORMAT>.json,
//                                    signed with the updater's key (TAURI_SIGNING_PRIVATE_KEY,
//                                    as the Packs workflow has it), once its serial is newer
//                                    than the published one
//   node scripts/packs.mjs published fails unless every file of catalog.json is on the
//                                    `packs` release (before an app release: its built-in
//                                    catalog points there)
//   node scripts/packs.mjs serve [--slow]
//                                    serves the packs from src-tauri/models on
//                                    http://127.0.0.1:8790, to try downloads with a debug
//                                    build run with FLIPFILE_PACKS_URL=http://127.0.0.1:8790
//                                    (--slow: at about 5 MB/s, to see the progress)
//
// A changed model: put it in src-tauri/models, run `catalog`; the serial goes up by one
// when anything changed. A change the app's code must follow (new inputs, a model it
// can't run as before) needs a new catalog format instead: FORMAT here and in
// src-tauri/src/packs/catalog.rs, so apps still on the old format keep the old catalog.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, createReadStream, existsSync, linkSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const FORMAT = 1;
const REPO = "wcksnrdn/flipfile";
const TAG = "packs";
const BASE = `https://github.com/${REPO}/releases/download/${TAG}`;
const PORT = 8790;

// Each pack: the ring targets it brings (format ids, or ring ids for a whole sub-ring), and its models (a Core ML model is a folder)
// for macOS (Core ML), Windows and Linux (ONNX), or every system
const PACKS = [
  {
    id: "upscale",
    name: "Upscale",
    targets: ["up-2x", "up-4x", "up-plain-2x", "up-plain-4x"],
    models: {
      coreml: ["upscale.mlmodelc", "upscale-photo.mlmodelc", "faces.mlmodelc", "faces-upscale.mlmodelc"],
      onnx: ["upscale.onnx", "upscale-photo.onnx", "faces.onnx", "faces-upscale.onnx", "yunet.onnx"],
    },
  },
  {
    id: "upscale-text",
    name: "Upscale Text",
    targets: ["up-text-2x", "up-text-4x"],
    models: {
      coreml: ["upscale-text.mlmodelc", "dejpeg.mlmodelc"],
      onnx: ["upscale-text.onnx", "dejpeg.onnx"],
    },
  },
  {
    id: "colorize",
    name: "Colorize",
    targets: ["colorize"],
    models: { coreml: ["colorize.mlmodelc"], onnx: ["colorize.onnx"] },
  },
  {
    id: "transcribe",
    name: "Transcribe",
    // Its rings, and its formats, which a Custom ring may hold by themselves
    targets: ["music-transcribe", "video-transcribe", "tr-srt", "tr-vtt", "tr-txt", "tr-docx", "tr-md"],
    models: { all: ["whisper-small.bin", "whisper-vad.bin"] },
  },
];

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const MODELS = join(ROOT, "src-tauri", "models");
const CATALOG = join(ROOT, "src-tauri", "packs", "catalog.json");

const sha256 = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");

/** Every file of a model (a folder's, in a stable order), relative to src-tauri/models */
function filesOf(model) {
  const path = join(MODELS, model);
  if (!existsSync(path)) throw new Error(`${model} is not in src-tauri/models: run npm run fetch-models (FLIPFILE_ONNX=1 on a Mac)`);
  if (!statSync(path).isDirectory()) return [model];
  return readdirSync(path, { recursive: true })
    .map((entry) => join(path, entry))
    .filter((file) => statSync(file).isFile() && !file.split(/[/\\]/).pop().startsWith("."))
    .map((file) => relative(MODELS, file).split("\\").join("/"))
    .sort();
}

/** The packs as src-tauri/models has them */
const packsOf = () =>
  PACKS.map(({ id, name, targets, models }) => ({
    id,
    name,
    targets,
    files: Object.fromEntries(
      Object.entries(models).map(([system, list]) => [
        system,
        list.flatMap(filesOf).map((path) => ({ path, sha256: sha256(join(MODELS, path)), size: statSync(join(MODELS, path)).size })),
      ]),
    ),
  }));

const readCatalog = () => (existsSync(CATALOG) ? JSON.parse(readFileSync(CATALOG, "utf8")) : null);

function catalog() {
  const previous = readCatalog();
  const packs = packsOf();
  const same = previous?.format === FORMAT && JSON.stringify(previous.packs) === JSON.stringify(packs);
  const serial = same ? previous.serial : (previous?.format === FORMAT ? previous.serial : 0) + 1;
  writeFileSync(CATALOG, `${JSON.stringify({ format: FORMAT, serial, base: BASE, packs }, null, 2)}\n`);
  for (const pack of packs) {
    const sizes = Object.entries(pack.files).map(([system, files]) => `${system} ${(files.reduce((n, f) => n + f.size, 0) / 1e6).toFixed(0)} MB`);
    console.log(`${pack.id}: ${sizes.join(", ")}`);
  }
  console.log(same ? `catalog.json unchanged (serial ${serial})` : `catalog.json written, serial ${serial}`);
}

function check() {
  const current = readCatalog();
  if (current?.format !== FORMAT || current.base !== BASE || JSON.stringify(current.packs) !== JSON.stringify(packsOf())) {
    throw new Error("catalog.json doesn't match src-tauri/models: run node scripts/packs.mjs catalog");
  }
  console.log(`catalog.json matches the models (serial ${current.serial})`);
  return current;
}

/** Every file of the catalog, once each (the same file may be in two packs) */
const uniqueFiles = (catalog) => {
  const files = new Map();
  for (const pack of catalog.packs) for (const list of Object.values(pack.files)) for (const file of list) files.set(file.sha256, file);
  return [...files.values()];
};

const gh = (...args) => execFileSync("gh", args, { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] });

function publish() {
  const catalog = check();
  const json = readFileSync(CATALOG);
  let assets;
  try {
    assets = JSON.parse(gh("release", "view", TAG, "--repo", REPO, "--json", "assets")).assets;
  } catch {
    // Never the latest release: the in-app updater and the install scripts read that one
    gh("release", "create", TAG, "--repo", REPO, "--prerelease", "--latest=false", "--title", "AI feature packs",
      "--notes", "The models flipfile downloads from Workshop, one feature at a time. Each file is named by its SHA-256; catalog-*.json lists them and is signed with the updater's key.");
    assets = [];
  }
  const sizes = new Map(assets.map((a) => [a.name, a.size]));

  const work = mkdtempSync(join(tmpdir(), "flipfile-packs-"));
  try {
    const files = uniqueFiles(catalog);
    const missing = files.filter((f) => !sizes.has(f.sha256));
    for (const file of files) {
      if (sizes.has(file.sha256) && sizes.get(file.sha256) !== file.size) {
        throw new Error(`the published ${file.sha256} (${file.path}) is ${sizes.get(file.sha256)} bytes, not ${file.size}: delete that asset and publish again`);
      }
    }
    console.log(`${files.length - missing.length} of ${files.length} files published already`);
    for (const [i, file] of missing.entries()) {
      // Uploaded under its hash: a link (or copy) by that name
      const named = join(work, file.sha256);
      try {
        linkSync(join(MODELS, file.path), named);
      } catch {
        copyFileSync(join(MODELS, file.path), named);
      }
      console.log(`[${i + 1}/${missing.length}] ${file.path} (${(file.size / 1e6).toFixed(1)} MB)`);
      gh("release", "upload", TAG, named, "--repo", REPO);
      rmSync(named);
    }

    const name = `catalog-${FORMAT}.json`;
    let published = null;
    if (sizes.has(name)) {
      gh("release", "download", TAG, "--repo", REPO, "--pattern", name, "--dir", work);
      published = readFileSync(join(work, name));
      rmSync(join(work, name));
    }
    const serial = published && JSON.parse(published.toString("utf8")).serial;
    if (published && published.equals(json)) {
      console.log(`${name} is published already (serial ${serial})`);
      return;
    }
    if (published && serial >= catalog.serial) {
      throw new Error(`the published ${name} has serial ${serial}, this one ${catalog.serial}: run node scripts/packs.mjs catalog after a change`);
    }
    if (!process.env.TAURI_SIGNING_PRIVATE_KEY) {
      console.log(`Files published. ${name} (serial ${catalog.serial}) needs the updater's key to be signed: the Packs workflow does it.`);
      return;
    }
    writeFileSync(join(work, name), json);
    execFileSync(process.platform === "win32" ? "npx.cmd" : "npx", ["tauri", "signer", "sign", join(work, name)], { stdio: "inherit" });
    // The signature first: an app reading between the two uploads finds the old catalog
    // with a signature that doesn't fit, ignores both and looks again later
    gh("release", "upload", TAG, join(work, `${name}.sig`), join(work, name), "--repo", REPO, "--clobber");
    console.log(`${name} published, serial ${catalog.serial}`);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

function published() {
  const catalog = readCatalog();
  const assets = JSON.parse(gh("release", "view", TAG, "--repo", REPO, "--json", "assets")).assets;
  const sizes = new Map(assets.map((a) => [a.name, a.size]));
  const missing = uniqueFiles(catalog).filter((f) => sizes.get(f.sha256) !== f.size);
  if (missing.length > 0) {
    throw new Error(`not on the packs release yet: ${missing.map((f) => f.path).join(", ")}. Run the Packs workflow first.`);
  }
  console.log(`every file of catalog.json (serial ${catalog.serial}) is published`);
}

/** The packs over plain HTTP, as GitHub serves them (ranges too), for a debug build */
function serve(slow) {
  const files = new Map(uniqueFiles(readCatalog()).map((f) => [f.sha256, f]));
  createServer((req, res) => {
    const file = files.get(req.url.slice(1));
    if (!file || !existsSync(join(MODELS, file.path))) {
      res.writeHead(404).end();
      return;
    }
    const range = /^bytes=(\d+)-$/.exec(req.headers.range ?? "");
    const start = range ? Number(range[1]) : 0;
    res.writeHead(range ? 206 : 200, { "content-length": file.size - start, "content-type": "application/octet-stream" });
    console.log(`${req.method} ${file.path}${start ? ` from ${start}` : ""}`);
    const stream = createReadStream(join(MODELS, file.path), { start, highWaterMark: slow ? 256 * 1024 : 1 << 20 });
    if (!slow) {
      stream.pipe(res);
      return;
    }
    // ~5 MB/s: a 256 KB chunk every 50 ms
    stream.on("data", (chunk) => {
      stream.pause();
      res.write(chunk);
      setTimeout(() => stream.resume(), 50);
    });
    stream.on("end", () => res.end());
    res.on("close", () => stream.destroy());
  }).listen(PORT, "127.0.0.1", () => {
    console.log(`Serving ${files.size} files from src-tauri/models on http://127.0.0.1:${PORT}${slow ? " (slow)" : ""}`);
    console.log(`Run the app with: FLIPFILE_PACKS_URL=http://127.0.0.1:${PORT} npm run tauri dev`);
  });
}

const [command, ...args] = process.argv.slice(2);
try {
  if (command === "catalog") catalog();
  else if (command === "check") check();
  else if (command === "publish") publish();
  else if (command === "published") published();
  else if (command === "serve") serve(args.includes("--slow"));
  else {
    console.error("usage: node scripts/packs.mjs catalog | check | publish | published | serve [--slow]");
    process.exitCode = 1;
  }
} catch (e) {
  console.error(`\npacks: ${e.message}\n`);
  process.exitCode = 1;
}
