// The Photo Editor's window sizes: S / M / L beside the circle, or the user's own from
// the corner grip. The last size chosen is remembered for the next photo.

import { preset, type PanelSizing, type Size } from "../workshop/workshopWindow";

export const EDITOR_SIZING: PanelSizing = {
  presets: [
    preset("small", "S", "Small", { width: 900, height: 580 }),
    preset("medium", "M", "Medium", { width: 1040, height: 660 }),
    preset("large", "L", "Large", { width: 1260, height: 800 }),
  ],
  min: { width: 860, height: 540 },
  max: { width: 1800, height: 1200 },
};

const DEFAULT_PRESET = "medium";
const STORAGE_KEY = "flipfile.editor.size.v1";

export type EditorSize = { size: Size; preset: string };

const fallback = (): EditorSize => {
  const p = EDITOR_SIZING.presets.find((p) => p.id === DEFAULT_PRESET)!;
  return { size: p.size, preset: p.id };
};

const valid = (n: unknown, min: number, max: number): n is number =>
  typeof n === "number" && Number.isFinite(n) && n >= min && n <= max;

/** The size the editor opens at */
export function loadEditorSize(): EditorSize {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return fallback();
    const { size, preset } = JSON.parse(raw) as Partial<EditorSize>;
    const { min, max } = EDITOR_SIZING;
    const known = preset === "" || EDITOR_SIZING.presets.some((p) => p.id === preset);
    if (!known || !size || !valid(size.width, min.width, max.width) || !valid(size.height, min.height, max.height)) {
      return fallback();
    }
    return { size, preset: preset ?? "" };
  } catch {
    return fallback();
  }
}

export function saveEditorSize(size: Size, preset: string) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ size, preset }));
  } catch {
    // Not remembered this time: the editor still opens at its default size
  }
}
