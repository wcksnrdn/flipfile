import { COMPRESS_LEVELS, labelOf } from "../../rings/dropTarget";
import { compressToast, failureToast, formatBytes, type ToastData } from "../../toast/toast";
import { OFFICE_FILE, SAME_FORMAT, STATUS_CLEAR_MS } from "./constants";
import type { BatchResult, Progress, Status } from "./types";
import { plural } from "./utils";

// Pure descriptions of a conversion batch: what the circle says while it runs, how its
// progress reads, and what the circle and the bubble beside it say once it is done.

type BatchJob = { paths: string[]; format: string; preset: string | null };

const labelsOf = ({ format, preset }: BatchJob) => {
  const same = format === SAME_FORMAT;
  const presetLabel = preset && !same ? ` · ${labelOf(preset)}` : "";
  const label = same ? (labelOf(preset ?? "") ?? "Done") : (labelOf(format) ?? format.toUpperCase()) + presetLabel;
  return { same, presetLabel, label };
};

const verbOf = (format: string) =>
  format.startsWith("up-")
    ? "Upscaling"
    : format === "colorize"
      ? "Colorizing"
      : format.startsWith("tr-")
        ? "Listening"
        : COMPRESS_LEVELS.includes(format)
          ? "Compressing"
          : "Converting";

export function batchStartStatus(job: BatchJob): Status {
  const many = job.paths.length > 1;
  return {
    title: many ? `0/${job.paths.length}` : verbOf(job.format),
    hint: `→ ${labelsOf(job).label}`,
    tone: "busy",
    progress: many ? 0 : undefined,
  };
}

/** Folds one progress report into a running batch's status */
export function batchProgressStatus(s: Status | null, { done, total, unit }: Progress, many: boolean): Status | null {
  if (s?.tone !== "busy") return s;
  // A single PDF reports its pages, a single upscale its steps; several files report files
  if (unit === "page" && total > 1) return { ...s, title: `Page ${done}/${total}`, progress: done / total };
  if (unit === "step" && total > 0) return { ...s, title: `${Math.floor((done / total) * 100)}%`, progress: done / total };
  if (unit === "file" && many) return { ...s, title: `${done}/${total}`, progress: done / total };
  return s;
}

export type BatchReport = {
  status: Status;
  clearAfterMs: number;
  // The bubble beside the circle: what failed and why, or what a compression gained and cost
  toast: ToastData | null;
};

export function batchReport(job: BatchJob, result: BatchResult): BatchReport {
  const { paths, format, preset } = job;
  const { outputs, errors, pages, bytes_before, bytes_after, note } = result;
  const { same, presetLabel, label } = labelsOf(job);
  const many = paths.length > 1;

  const merged = format === "pdf" && many && outputs.length === 1;
  const mergedGif = format === "gif-anim" && outputs.length === 1;
  const mergedSlides =
    format === "pptx" && many && outputs.length === 1 && paths.every((p) => !OFFICE_FILE.test(p) && !/\.pdf$/i.test(p));
  // Resizing/compressing in place: the size change is what the user wants to see
  const sizes = (same || preset === "compress" || format.startsWith("cmp-") || format === "vid-compress") && bytes_before > 0;
  // A compress level: the sizes and the saving, and a format change if there was one
  const level = COMPRESS_LEVELS.includes(format) && bytes_before > 0;
  const formatChange = note?.match(/^[A-Z0-9]+ → [A-Z0-9]+/)?.[0];
  const succeeded = paths.length - errors.length;

  const done = level
    ? bytes_after >= bytes_before
      ? `Already compact · ${formatBytes(bytes_before)}`
      : `${formatBytes(bytes_before)} → ${formatBytes(bytes_after)} · −${Math.round((1 - bytes_after / bytes_before) * 100)}%${formatChange ? ` · ${formatChange}` : ""}`
    : sizes
      ? `${note ? `${note} · ` : bytes_after >= bytes_before ? "Already compact · " : ""}${formatBytes(bytes_before)} → ${formatBytes(bytes_after)}`
      : merged
        ? `${plural(succeeded, "image")} → 1 PDF${presetLabel}`
        : mergedGif
          ? `${plural(succeeded, "image")} → GIF${presetLabel}`
          : mergedSlides
            ? `${plural(succeeded, "image")} → 1 presentation`
            : pages > 0 // PDF pages rendered to images (multi-page PDFs give a folder each)
              ? `${plural(pages, "page")} → ${label}`
              : `${plural(outputs.length, "file")} → ${label}`;

  // A note (e.g. which Word engine was used) is shown unless it is already part of `done`
  const extra = note && !sizes ? ` · ${note}` : "";
  const status: Status =
    errors.length === 0
      ? { title: "✓ Done", hint: `${done}${extra} · click to show`, tone: "ok", outputs }
      : {
          title: outputs.length > 0 ? `${succeeded} done, ${errors.length} failed` : "Failed",
          hint: errors[0],
          tone: "err",
          outputs,
        };

  const compressing =
    COMPRESS_LEVELS.includes(format) ||
    format.startsWith("cmp-") ||
    format === "vid-compress" ||
    (same && (preset === "compress" || !!preset));

  let toast: ToastData | null = null;
  if (errors.length > 0) {
    toast = failureToast(errors, paths.length, outputs);
  } else if (compressing && bytes_before > 0) {
    const title = COMPRESS_LEVELS.includes(format)
      ? `Compressed · ${labelOf(format)?.replace("Compress · ", "")}`
      : same
        ? (labelOf(preset ?? "") ?? "Resized")
        : (labelOf(format) ?? "Compressed");
    toast = compressToast({ title, format, before: bytes_before, after: bytes_after, note, files: outputs.length, outputs });
  }

  return {
    status,
    clearAfterMs: outputs.length > 0 ? STATUS_CLEAR_MS.result : STATUS_CLEAR_MS.failure,
    toast,
  };
}

/** A batch that never reached the backend (the command itself threw) */
export const failedBatch = (error: unknown): BatchResult => ({
  outputs: [],
  errors: [String(error)],
  pages: 0,
  bytes_before: 0,
  bytes_after: 0,
  note: null,
});
