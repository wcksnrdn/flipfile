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
//
// A changed model: put it in src-tauri/models, run `catalog`; the serial goes up by one
// when anything changed. A change the app's code must follow (new inputs, a model it
// can't run as before) needs a new catalog format instead: FORMAT here and in
// src-tauri/src/packs/catalog.rs, so apps still on the old format keep the old catalog.

import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const FORMAT = 1;
const BASE = "https://github.com/wcksnrdn/flipfile/releases/download/packs";

// Each pack: the ring targets it brings, and its models (a Core ML model is a folder)
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
    targets: ["music-transcribe", "video-transcribe"],
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

function catalog() {
  const previous = existsSync(CATALOG) ? JSON.parse(readFileSync(CATALOG, "utf8")) : null;
  const packs = PACKS.map(({ id, name, targets, models }) => ({
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
  const same = previous?.format === FORMAT && JSON.stringify(previous.packs) === JSON.stringify(packs);
  const serial = same ? previous.serial : (previous?.format === FORMAT ? previous.serial : 0) + 1;
  writeFileSync(CATALOG, `${JSON.stringify({ format: FORMAT, serial, base: BASE, packs }, null, 2)}\n`);
  for (const pack of packs) {
    const sizes = Object.entries(pack.files).map(([system, files]) => `${system} ${(files.reduce((n, f) => n + f.size, 0) / 1e6).toFixed(0)} MB`);
    console.log(`${pack.id}: ${sizes.join(", ")}`);
  }
  console.log(same ? `catalog.json unchanged (serial ${serial})` : `catalog.json written, serial ${serial}`);
}

const command = process.argv[2];
if (command === "catalog") catalog();
else {
  console.error("usage: node scripts/packs.mjs catalog");
  process.exitCode = 1;
}
