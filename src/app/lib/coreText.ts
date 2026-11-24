import { COMPRESS_LEVELS, isCategoryRing, labelOf, type Ring, type RingId } from "../../rings/dropTarget";
import { formatBytes } from "../../toast/toast";
import { AI_SUMMARY, PHOTO_EDITOR, type WorkspaceConfig } from "../../workshop/workshop";
import { ALT_KEY, ALT_SUPPORTED, CATEGORY_LABELS, ON_DEVICE } from "./constants";
import type { ChipDrag, Estimate, Picking, Status } from "./types";
import { plural } from "./utils";

/** The three lines of text in the core: a title, a hint under it, and an optional badge */
export type CoreText = { title: string; hint: string; badge: string | null };

export type CoreTextInput = {
  open: boolean;
  dragging: boolean;
  picking: Picking | null;
  workshopOpen: boolean;
  chipDrag: ChipDrag | null;
  ringId: RingId;
  rings: Record<RingId, Ring>;
  hovered: string | null;
  preset: string | null;
  estimates: Record<string, Estimate>;
  fileCount: number;
  mergeable: boolean;
  settingsSummary: string | null;
  activeWorkspace: WorkspaceConfig;
  status: Status | null;
  /** The name of the photo open in the Photo Editor beside the circle */
  editing: string | null;
};

type Hand = { fileCount: number; mergeable: boolean };

const is =
  (...ids: string[]) =>
  (id: string) =>
    ids.includes(id);
const startsWith =
  (...prefixes: string[]) =>
  (id: string) =>
    prefixes.some((p) => id.startsWith(p));
const merging = ({ fileCount, mergeable }: Hand) => fileCount > 1 && mergeable;

// What releasing (or clicking) on a slice does, after "Release"/"Click". First match wins.
const ACTION_HINTS: { when: (id: string, hand: Hand) => boolean; does: string }[] = [
  { when: is(PHOTO_EDITOR), does: "to open in Photo Editor" },
  { when: is("clean"), does: "to strip metadata" },
  { when: is("nobg"), does: "to remove background" },
  { when: is("colorize"), does: "to colour with AI" },
  { when: startsWith("up-"), does: "to enlarge with AI" },
  { when: is("tr-srt", "tr-vtt"), does: "to make subtitles with AI" },
  { when: startsWith("tr-"), does: "to write down what is said" },
  { when: is("square"), does: "to pad to 1:1 square" },
  { when: is("gif-anim"), does: "to combine into animated GIF" },
  { when: (id, hand) => id === "pptx" && merging(hand), does: "to combine into 1 presentation" },
  { when: is("pptx"), does: "to make PowerPoint slides" },
  { when: is("ico"), does: "for multi-size .ico" },
  { when: is("icns"), does: "for macOS .icns icon" },
  { when: startsWith("cmp-"), does: "to compress file size" },
  { when: (id) => startsWith("rot-", "flip-")(id) || id === "grayscale", does: "to transform photo" },
  { when: is("docx-ocr"), does: "to read scanned text into Word" },
  { when: is(AI_SUMMARY), does: "to explain it with AI" },
  { when: is("vid-compress"), does: "to shrink video file size" },
  { when: is("vid-mute"), does: "to strip audio track" },
  { when: is("vid-mp3"), does: "to extract audio track" },
  { when: is("vid-gif"), does: "to convert video clip to GIF" },
  { when: is("vid-1080p", "vid-720p"), does: "to resize video resolution" },
  { when: is("sql"), does: "to generate SQL table & inserts" },
  { when: is("md-table"), does: "to export Markdown table" },
  { when: (id, hand) => id === "pdf" && merging(hand), does: "to merge into 1 PDF" },
];

const actionHint = (id: string, hand: Hand) => ACTION_HINTS.find((h) => h.when(id, hand))?.does ?? "to convert";

// Upscaling sets its own size, so size presets don't apply to it (nor to transcripts)
const isAiTool = (id: string | null) => id === AI_SUMMARY || !!id?.startsWith("up-") || !!id?.startsWith("tr-");

function workshopText({ chipDrag, hovered, activeWorkspace, ringId }: CoreTextInput): CoreText {
  const badge = "Workshop";
  if (chipDrag) {
    const hint =
      chipDrag.targetSlice === "core"
        ? "+ Add to ring"
        : typeof chipDrag.targetSlice === "number"
          ? `Replace slot #${chipDrag.targetSlice + 1}`
          : "Drop on a slice";
    return { title: chipDrag.item.label, hint, badge };
  }
  const hoveredLabel = hovered ? labelOf(hovered) : undefined;
  if (hoveredLabel) return { title: hoveredLabel, hint: "Drop to replace", badge };
  return {
    title: activeWorkspace.mode === "direct" ? activeWorkspace.name : (CATEGORY_LABELS[ringId] ?? "Ring"),
    hint: "Drop on a slice",
    badge,
  };
}

// A compress level: what it makes of the files, and what that costs
function compressLevelText(levelId: string, estimate: Estimate | undefined): CoreText {
  const level = labelOf(levelId)?.replace("Compress · ", "") ?? "";
  const badge = `Compress: ${level}`;
  if (estimate && estimate.after !== null && estimate.before > 0) {
    const saved = Math.round((1 - estimate.after / estimate.before) * 100);
    const approx = estimate.approximate ? "≈ " : "";
    return {
      title: saved > 0 ? `${formatBytes(estimate.before)} → ${approx}${formatBytes(estimate.after)}` : formatBytes(estimate.before),
      hint: saved > 0 ? `−${saved}% · ${estimate.note}` : "Already compact",
      badge,
    };
  }
  return { title: level, hint: estimate ? estimate.note : "Working out the size…", badge };
}

// The Option/Alt ring of size/quality presets
function presetRingText({ preset, dragging }: CoreTextInput): CoreText {
  const presetLabel = preset ? labelOf(preset) : undefined;
  return {
    title: presetLabel ?? "Compress or resize",
    hint: presetLabel ? `${dragging ? "Drop" : "Click"}: same format, or release ${ALT_KEY}` : "Point to a level or a size",
    badge: null,
  };
}

// Files in hand (dragged in or pasted), aiming at a ring
function aimingText(input: CoreTextInput): CoreText {
  const { hovered, preset, dragging, picking, ringId, rings, fileCount, mergeable, settingsSummary } = input;
  const verb = dragging ? "Release" : "Click";
  const presetLabel = preset ? labelOf(preset) : undefined;
  const badge = isAiTool(hovered)
    ? ON_DEVICE
    : presetLabel
      ? `Size: ${presetLabel}`
      : ALT_SUPPORTED
        ? `Hold ${ALT_KEY} to resize`
        : settingsSummary;

  // The lobby names its rooms; an action placed in it (AI Summary) speaks for itself
  const hoveredAction = !!hovered && !rings[ringId]?.items.find((i) => i.id === hovered)?.nav;
  if (isCategoryRing(ringId) && !hoveredAction) return { title: "Convert", hint: "Pick a category", badge };

  const hoveredLabel = hovered ? labelOf(hovered) : undefined;
  if (hovered && hoveredLabel) {
    return { title: hoveredLabel, hint: `${verb} ${actionHint(hovered, { fileCount, mergeable })}`, badge };
  }
  return {
    title: CATEGORY_LABELS[ringId] ?? (picking?.image ? "Image" : plural(fileCount, "file")),
    hint: dragging ? "Point to a format" : "Pick a format · Esc to cancel",
    badge,
  };
}

/** Works out what the core says for the current interaction, most specific state first */
export function coreText(input: CoreTextInput): CoreText {
  const { open, workshopOpen, hovered, ringId, status, estimates, settingsSummary, activeWorkspace, editing } = input;
  if (workshopOpen) return workshopText(input);
  // Beside the Photo Editor: what is being edited, or that it was just saved (click to show it)
  if (editing !== null) return status ? { title: status.title, hint: status.hint, badge: null } : { title: "Editing", hint: editing, badge: null };
  if (open && hovered && COMPRESS_LEVELS.includes(hovered)) return compressLevelText(hovered, estimates[hovered]);
  if (open && ringId === "size") return presetRingText(input);
  if (open) return aimingText(input);

  const badge = settingsSummary ?? (activeWorkspace.id !== "all-rooms" ? activeWorkspace.name : "Customize");
  if (status) return { title: status.title, hint: status.hint, badge };
  return { title: "Convert", hint: "Drop files here", badge };
}
