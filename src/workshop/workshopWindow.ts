import { invoke } from "@tauri-apps/api/core";

// The main window is the bare circle until a panel opens beside it: the Ring Workshop,
// or the Photo Editor (set_workshop_window in src-tauri). The sizes it takes are decided
// here and in src/editor/editorWindow.ts.

export type Size = { width: number; height: number };

export const COMPACT_SIZE = 320; // the bare circle, square
export const WORKSHOP_DEFAULT: Size = { width: 940, height: 620 };
export const WORKSHOP_MIN: Size = { width: 720, height: 480 };
export const WORKSHOP_MAX: Size = { width: 1800, height: 1200 };

export type WindowPreset = { id: string; label: string; size: Size; title: string };

/** How a panel's window may be sized: its S / M / L presets and its limits */
export type PanelSizing = { presets: WindowPreset[]; min: Size; max: Size };

export const preset = (id: string, label: string, name: string, size: Size): WindowPreset => ({
  id,
  label,
  size,
  title: `${name} Window (${size.width}×${size.height})`,
});

/** The S / M / L buttons in the Workshop header */
export const WINDOW_PRESETS = [
  preset("compact", "S", "Compact", { width: 820, height: 540 }),
  preset("comfort", "M", "Default", WORKSHOP_DEFAULT),
  preset("large", "L", "Large", { width: 1120, height: 720 }),
];
export const DEFAULT_PRESET = "comfort";
export const WORKSHOP_SIZING: PanelSizing = { presets: WINDOW_PRESETS, min: WORKSHOP_MIN, max: WORKSHOP_MAX };

const clamp = (n: number, min: number, max: number) => Math.round(Math.max(min, Math.min(max, n)));

// Each change waits for the one before it: closing one panel and opening another at
// once (the Workshop, then the Photo Editor) must not interleave their steps
let resizing: Promise<unknown> = Promise.resolve();

/** Resizes the window between the bare circle and a panel (the Workshop, the Photo Editor);
 *  resolves once the window has its new size on screen */
export function applyPanelWindow(open: boolean, size: Size = WORKSHOP_DEFAULT, min: Size = WORKSHOP_MIN): Promise<void> {
  const { width, height } = open ? size : { width: COMPACT_SIZE, height: COMPACT_SIZE };
  // All in one backend call, which keeps the window's top-left corner (and so the circle) in place
  const apply = () =>
    invoke("set_workshop_window", { open, width, height, minWidth: min.width, minHeight: min.height }).then(() =>
      settled(width, height)
    );
  const done = resizing.then(apply);
  resizing = done.catch(() => {});
  return done.catch(() => {});
}

// The backend resizes the window a moment after it answers: wait until the page has the
// new size, and one frame more for it to be drawn (at most SETTLE_LIMIT_MS)
const SETTLE_LIMIT_MS = 250;
function settled(width: number, height: number): Promise<void> {
  const start = performance.now();
  return new Promise((resolve) => {
    const check = () => {
      const there = Math.abs(window.innerWidth - width) <= 2 && Math.abs(window.innerHeight - height) <= 2;
      if (there || performance.now() - start > SETTLE_LIMIT_MS) requestAnimationFrame(() => resolve());
      else requestAnimationFrame(check);
    };
    check();
  });
}

/** Resizes an open panel's window, kept within its limits; resolves once the window has the size */
export function resizePanelWindow(width: number, height: number, { min, max }: PanelSizing): Promise<Size> {
  const size = { width: clamp(width, min.width, max.width), height: clamp(height, min.height, max.height) };
  return invoke("set_workshop_window", { open: true, ...size, minWidth: min.width, minHeight: min.height }).then(() => size);
}
