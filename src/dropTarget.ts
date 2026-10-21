import type { PhysicalPosition } from "@tauri-apps/api/dpi";

// Single source of truth for the ring layout. Both the segment layout (App.tsx)
// and the drop hit-testing below are derived from these values, so they cannot drift.

export type RingId = "main" | "more" | "size";

export type Item = {
  id: string;
  label: string;
  sub?: string;
  disabled?: boolean;
  nav?: RingId; // hovering switches to this ring instead of selecting a format
  preset?: boolean; // size/quality preset (Option/Alt ring), picked by hovering
};

export type Ring = {
  id: RingId;
  items: Item[];
  start: number; // angle of the first item's center
  sector: number;
};

const ring = (id: RingId, items: Item[], start: number): Ring => ({
  id,
  items,
  start,
  sector: (2 * Math.PI) / items.length,
});

const MAIN = ring(
  "main",
  [
    { id: "jpg", label: "JPG" },
    { id: "jpeg", label: "JPEG" },
    { id: "png", label: "PNG" },
    { id: "webp", label: "WEBP" },
    { id: "bmp", label: "BMP" },
    { id: "pdf", label: "PDF" },
    { id: "docx", label: "WORD", sub: "Soon", disabled: true },
    { id: "more", label: "More", sub: "•••", nav: "more" },
  ],
  -Math.PI / 2 // first format sits straight up
);

const MORE_ITEMS: Item[] = [
  { id: "tiff", label: "TIFF" },
  { id: "gif", label: "GIF" },
  { id: "ico", label: "ICO" },
  { id: "avif", label: "AVIF" },
  { id: "clean", label: "Clean", sub: "metadata" },
  { id: "back", label: "Back", sub: "‹", nav: "main" },
];

// "Back" sits in exactly the same direction as "More", so the way back is
// where the user came from.
const moreAngle = MAIN.start + MAIN.items.findIndex((i) => i.nav === "more") * MAIN.sector;
const backIndex = MORE_ITEMS.findIndex((i) => i.nav === "main");
const MORE = ring("more", MORE_ITEMS, moreAngle - backIndex * ((2 * Math.PI) / MORE_ITEMS.length));

// Shown while Option/Alt is held during a drag. Ids match the presets in settings.rs.
const SIZE = ring(
  "size",
  [
    { id: "original", label: "Original", sub: "size", preset: true },
    { id: "half", label: "50%", preset: true },
    { id: "4k", label: "4K", sub: "3840 px", preset: true },
    { id: "1080p", label: "1080p", sub: "1920 px", preset: true },
    { id: "720p", label: "720p", sub: "1280 px", preset: true },
    { id: "compress", label: "Compress", sub: "smaller", preset: true },
  ],
  -Math.PI / 2
);

export const RINGS: Record<RingId, Ring> = { main: MAIN, more: MORE, size: SIZE };

export function labelOf(id: string): string | undefined {
  for (const r of Object.values(RINGS)) {
    const item = r.items.find((i) => i.id === id);
    if (item) return item.label;
  }
}

export const STAGE_SIZE = 300;
export const CORE_SIZE = 120;
export const CORE_OPEN_SCALE = 0.86; // core shrinks while a drag is in progress

// Dropping inside the (shrunken) core never selects anything.
export const DEAD_ZONE = (CORE_SIZE / 2) * CORE_OPEN_SCALE;

// Ring of slice-shaped segments around the core.
export const RING_INNER = DEAD_ZONE + 6;
export const RING_OUTER = 118;
export const LABEL_RADIUS = (RING_INNER + RING_OUTER) / 2;
export const SEGMENT_GAP = 3; // px between neighbouring segments
export const SEGMENT_CORNER = 7; // px corner rounding
export const HOVER_POP = 5; // px the active segment moves outward

export const angleOf = (r: Ring, index: number) => r.start + index * r.sector;

export type Point = { x: number; y: number };

// Tauri types drag-drop positions as PhysicalPosition, but the underlying wry
// backends do not agree on units (verified against wry 0.57 / tauri 2.12):
// - Windows (WebView2): ScreenToClient() -> real physical pixels, so divide by the scale factor.
// - macOS (WKWebView): NSDraggingInfo.draggingLocation -> points, i.e. already logical
//   (dividing by devicePixelRatio halves the position on Retina; this was the drop bug).
// - Linux (WebKitGTK): GTK widget coordinates -> already logical.
// Detected via the user agent to avoid pulling in @tauri-apps/plugin-os for one check.
const POSITION_IS_PHYSICAL = /Windows/i.test(navigator.userAgent);

export function toCssPoint(position: PhysicalPosition, scaleFactor: number): Point {
  if (POSITION_IS_PHYSICAL) {
    const p = position.toLogical(scaleFactor);
    return { x: p.x, y: p.y };
  }
  return { x: position.x, y: position.y };
}

export type Zone = { kind: "core" } | { kind: "item"; index: number; item: Item };

export type ZoneDetail = { zone: Zone; dx: number; dy: number; distance: number; angleDeg: number };

// Sector-based selection: outside the core, whichever item's angular slice
// contains the cursor is picked, so the user only has to move toward a segment.
export function zoneAt(point: Point, center: Point, r: Ring): ZoneDetail {
  const dx = point.x - center.x;
  const dy = point.y - center.y;
  const distance = Math.hypot(dx, dy);

  // Angle measured from the first segment's center, normalized to [0, 2π).
  let rel = (Math.atan2(dy, dx) - r.start) % (2 * Math.PI);
  if (rel < 0) rel += 2 * Math.PI;
  // Each label sits in the middle of its slice, so round instead of floor.
  const index = Math.round(rel / r.sector) % r.items.length;
  const angleDeg = (rel * 180) / Math.PI;

  const zone: Zone = distance <= DEAD_ZONE ? { kind: "core" } : { kind: "item", index, item: r.items[index] };
  return { zone, dx, dy, distance, angleDeg };
}

type Vec2 = [number, number];
const polar = (r: number, a: number): Vec2 => [Math.cos(a) * r, Math.sin(a) * r];
const lerp = (p: Vec2, q: Vec2, t: number): Vec2 => [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
const xy = (p: Vec2) => `${p[0].toFixed(2)} ${p[1].toFixed(2)}`;

// SVG path for one slice of a ring, centered on (0, 0), with a constant pixel
// gap to its neighbours and rounded corners.
export function segmentPath(
  r: Ring,
  index: number,
  inner = RING_INNER,
  outer = RING_OUTER,
  gap = SEGMENT_GAP,
  corner = SEGMENT_CORNER
): string {
  const mid = angleOf(r, index);
  // Angular half-width at radius rad, so the straight gap stays `gap` px wide everywhere.
  const half = (rad: number) => r.sector / 2 - Math.asin(gap / 2 / rad);

  const o0 = polar(outer, mid - half(outer));
  const o1 = polar(outer, mid + half(outer));
  const i1 = polar(inner, mid + half(inner));
  const i0 = polar(inner, mid - half(inner));
  const side = Math.hypot(o1[0] - i1[0], o1[1] - i1[1]);
  const t = corner / side;

  return [
    `M ${xy(polar(outer, mid - half(outer) + corner / outer))}`,
    `A ${outer} ${outer} 0 0 1 ${xy(polar(outer, mid + half(outer) - corner / outer))}`,
    `Q ${xy(o1)} ${xy(lerp(o1, i1, t))}`,
    `L ${xy(lerp(i1, o1, t))}`,
    `Q ${xy(i1)} ${xy(polar(inner, mid + half(inner) - corner / inner))}`,
    `A ${inner} ${inner} 0 0 0 ${xy(polar(inner, mid - half(inner) + corner / inner))}`,
    `Q ${xy(i0)} ${xy(lerp(i0, o0, t))}`,
    `L ${xy(lerp(o0, i0, t))}`,
    `Q ${xy(o0)} ${xy(polar(outer, mid - half(outer) + corner / outer))}`,
    "Z",
  ].join(" ");
}
