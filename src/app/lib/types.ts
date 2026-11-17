import type { CatalogEntry } from "../../workshop/workshop";

/** What the circle shows between drops: a conversion's progress, its result, or a notice */
export type Status = {
  title: string;
  hint: string;
  tone: "busy" | "ok" | "err";
  progress?: number; // 0..1 when known (several files, or the pages of one PDF); else a spinner
  outputs?: string[];
  ai?: boolean; // an AI summary: the panel shows the result, clicking the circle brings it back
};

// What the summary panel reports to the circle (src-tauri/src/summary/mod.rs)
export type SummaryProgress = {
  title: string;
  hint: string;
  tone: "busy" | "ok" | "err" | "idle";
  progress: number | null;
};

// Files or an image pasted with Cmd/Ctrl+V, waiting for the user to click a format.
export type Picking = {
  paths: string[];
  image: boolean; // raw clipboard image (a temp file): the backend saves its result in Downloads
};

export type Pasted = { kind: "files"; paths: string[] } | { kind: "image"; path: string } | { kind: "empty" };

/** One compress level's live estimate (src-tauri/src/convert/compress/estimate.rs) */
export type Estimate = {
  token: number;
  id: string;
  before: number;
  after: number | null;
  note: string;
  approximate: boolean;
};

export type BatchResult = {
  outputs: string[];
  errors: string[];
  pages: number;
  bytes_before: number;
  bytes_after: number;
  note: string | null; // e.g. "BMP can't be compressed further"
};

export type Progress = { done: number; total: number; unit: "file" | "page" | "step" };

/** Where a pointer position is being resolved from (drag & drop, paste-and-click, Option/Alt re-aim) */
export type ResolveType = "enter" | "over" | "drop" | "move" | "click";

/** A Workshop chip being dragged onto the ring or the slot list */
export type ChipDrag = {
  item: CatalogEntry;
  fromSlotIndex: number | null;
  x: number;
  y: number;
  targetSlice: number | "core" | null;
};

/** Runs a format (or AI Summary) on files: the shared end of a drop and a paste-and-click */
export type Execute = (paths: string[], format: string, toDownloads: boolean, preset: string | null) => void;
