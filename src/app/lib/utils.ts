import type { UnlistenFn } from "@tauri-apps/api/event";
import type { RingId } from "../../rings/dropTarget";
import type { WorkspaceConfig } from "../../workshop/workshop";
import { OFFICE_FILE } from "./constants";

export const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

// "Clipboard 2026-10-20 at 14.03.12", in local time (matches the macOS screenshot style)
export const clipboardName = (d = new Date()) => {
  const p = (n: number) => String(n).padStart(2, "0");
  return `Clipboard ${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} at ${p(d.getHours())}.${p(d.getMinutes())}.${p(d.getSeconds())}`;
};

// Several *images* dropped on PDF are merged into one; Word documents become one PDF each
export const isMergeable = (paths: string[]) => paths.length > 1 && paths.every((path) => !OFFICE_FILE.test(path));

// A "direct" workspace has a single ring to edit; a "rooms" one edits the room picked in the Workshop
export const editableRing = (mode: WorkspaceConfig["mode"], editingRing: RingId): RingId =>
  mode === "direct" ? "custom" : editingRing;

/**
 * Ties a Tauri subscription to an effect: returns the effect's cleanup, which also
 * unsubscribes when the component unmounts before the subscription has resolved.
 */
export function subscription(pending: Promise<UnlistenFn>): () => void {
  let cancelled = false;
  let unlisten: UnlistenFn | undefined;
  pending.then((fn) => (cancelled ? fn() : (unlisten = fn))).catch(() => {});
  return () => {
    cancelled = true;
    unlisten?.();
  };
}
