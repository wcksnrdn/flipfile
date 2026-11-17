import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

// The main window is the bare circle until the Ring Workshop opens beside it
// (set_workshop_window in src-tauri). Every size the window takes is decided here.

type Size = { width: number; height: number };

export const COMPACT_SIZE = 320; // the bare circle, square
export const WORKSHOP_DEFAULT: Size = { width: 940, height: 620 };
export const WORKSHOP_MIN: Size = { width: 720, height: 480 };
export const WORKSHOP_MAX: Size = { width: 1800, height: 1200 };

const preset = (id: string, label: string, name: string, size: Size) => ({
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

const clamp = (n: number, min: number, max: number) => Math.round(Math.max(min, Math.min(max, n)));

/** Resizes the window between the bare circle and the Workshop at its default size */
export function applyWorkshopWindow(open: boolean) {
  const { width, height } = open ? WORKSHOP_DEFAULT : { width: COMPACT_SIZE, height: COMPACT_SIZE };
  invoke("set_workshop_window", { open, width, height }).catch(() => {});
  const win = getCurrentWindow();
  const compact = new LogicalSize(COMPACT_SIZE, COMPACT_SIZE);
  const resizing = open
    ? win
        .setResizable(true)
        .then(() => win.setMinSize(new LogicalSize(WORKSHOP_MIN.width, WORKSHOP_MIN.height)))
        .then(() => win.setMaxSize(null))
        .then(() => win.setSize(new LogicalSize(width, height)))
    : win
        .setResizable(true)
        .then(() => win.setMinSize(compact))
        .then(() => win.setMaxSize(compact))
        .then(() => win.setSize(compact))
        .then(() => win.setResizable(false));
  resizing.catch(() => {});
}

/** Resizes the open Workshop window, kept within its limits; resolves once the window has the size */
export function resizeWorkshopWindow(width: number, height: number): Promise<void> {
  const w = clamp(width, WORKSHOP_MIN.width, WORKSHOP_MAX.width);
  const h = clamp(height, WORKSHOP_MIN.height, WORKSHOP_MAX.height);
  invoke("set_workshop_window", { open: true, width: w, height: h }).catch(() => {});
  return getCurrentWindow().setSize(new LogicalSize(w, h));
}
