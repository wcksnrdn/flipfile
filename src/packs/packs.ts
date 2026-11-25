import type { RingId } from "../rings/dropTarget";

// The AI feature packs (src-tauri/src/packs): the models of a feature, downloaded from the
// Workshop. A slice whose pack isn't here stays on the ring, saying what it takes to get it.

export type PackState = "missing" | "installed" | "update" | "downloading" | "restart";

/** A pack as the backend reports it (PackStatus in src-tauri/src/packs/mod.rs) */
export type PackStatus = {
  id: string;
  name: string;
  /** The formats and sub-rings that need it */
  targets: string[];
  state: PackState;
  /** The whole pack, in bytes */
  size: number;
  /** What is left to download for it */
  download: number;
  done: number;
  total: number;
  removable: boolean;
};

export type PackProgress = { id: string; done: number; total: number };

/** What each pack brings, in the Workshop */
export const PACK_INFO: Record<string, string> = {
  upscale: "Upscale Photo and Plain, 2× and 4×: sharper photos, faces restored",
  "upscale-text": "Upscale Text 2× and 4×: crisp documents, screenshots and chats",
  colorize: "Colorize: colour for black-and-white photos",
  transcribe: "Transcribe and Subtitles: what is said, as text or subtitles",
};

// The packs as last reported, read while the rings are built (like the AI support in
// workshop.ts): set by usePacks
let current: PackStatus[] = [];
export function setPacks(packs: PackStatus[]) {
  current = packs;
}

/** Whether its features work: there, even if a newer version is out */
export const usable = (pack: PackStatus) => pack.state === "installed" || pack.state === "update";

/** The pack a format (or a sub-ring, `nav`) needs and doesn't have yet */
export function packNeeded(id: string, nav?: RingId, packs: PackStatus[] = current): PackStatus | undefined {
  return packs.find((p) => (p.targets.includes(id) || (!!nav && p.targets.includes(nav))) && !usable(p));
}

/** A download's size, rounded as a slice has room for: "145 MB" */
export const sizeLabel = (bytes: number) => (bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(bytes / 1e6))} MB`);

export const percent = (done: number, total: number) => (total > 0 ? Math.min(99, Math.floor((done / total) * 100)) : 0);

/** The short line under a slice whose pack isn't here */
export function packSub(pack: PackStatus, progress?: PackProgress): string {
  if (pack.state === "downloading") return progress ? `${percent(progress.done, progress.total)}%` : "…";
  if (pack.state === "restart") return "restart";
  return `↓ ${sizeLabel(pack.download || pack.size)}`;
}
