import { invoke } from "@tauri-apps/api/core";
import { CORE_SIZE, HOVER_POP, RING_OUTER, type Point } from "../../rings/dropTarget";

// Native glass disc (macOS): core-sized while idle, covers the whole ring while dragging.
export const GLASS_IDLE = CORE_SIZE;
export const GLASS_OPEN = (RING_OUTER + HOVER_POP) * 2;

// Approximates the CSS spring cubic-bezier(0.34, 1.56, 0.64, 1) used by the segments.
export const easeOutBack = (t: number) => 1 + 2.2 * (t - 1) ** 3 + 1.2 * (t - 1) ** 2;
export const easeInOut = (t: number) => (t < 0.5 ? 4 * t ** 3 : 1 - (-2 * t + 2) ** 3 / 2);

/** Places the glass disc around `center`; resolves false where native glass is unavailable */
export const sendGlass = (center: Point, diameter: number) =>
  invoke<boolean>("set_glass", { cx: center.x, cy: center.y, diameter });
