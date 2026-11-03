import type { PhysicalPosition } from "@tauri-apps/api/dpi";

// Single source of truth for the ring layout. Both the segment layout (App.tsx)
// and the drop hit-testing below are derived from these values, so they cannot drift.

export type RingId =
  | "category"
  | "photo"
  | "photo-formats"
  | "photo-tools"
  | "photo-compress"
  | "photo-transform"
  | "office"
  | "video"
  | "music"
  | "size";

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

const IS_MAC = /Mac/i.test(navigator.userAgent);

const ring = (id: RingId, items: Item[], start: number): Ring => ({
  id,
  items,
  start,
  sector: (2 * Math.PI) / items.length,
});

// Places a sub-ring so its "Back" segment sits at the exact angle of the parent
// segment that opened it, so the way back is where the cursor came from.
const alignedSubRing = (id: RingId, items: Item[], parent: Ring, viaNav: RingId, backNav: RingId): Ring => {
  const enterIdx = parent.items.findIndex((i) => i.nav === viaNav);
  const enterAngle = parent.start + (enterIdx >= 0 ? enterIdx : 0) * parent.sector;
  const backIdx = items.findIndex((i) => i.nav === backNav);
  const sector = (2 * Math.PI) / items.length;
  return ring(id, items, enterAngle - (backIdx >= 0 ? backIdx : 0) * sector);
};

// ─── Category lobby (shown first on drag) ───────────────────────────────
const CATEGORY = ring(
  "category",
  [
    { id: "cat-photo", label: "Photo", nav: "photo" },
    { id: "cat-office", label: "Office", nav: "office" },
    { id: "cat-video", label: "Video", nav: "video" },
    { id: "cat-music", label: "Music", nav: "music" },
  ],
  -Math.PI / 2 // first category sits straight up
);

// ─── Photo room (main) ──────────────────────────────────────────────────
const PHOTO = ring(
  "photo",
  [
    { id: "jpg", label: "JPG" },
    { id: "png", label: "PNG" },
    { id: "webp", label: "WEBP" },
    { id: "heic", label: "HEIC", disabled: !IS_MAC },
    { id: "pdf", label: "PDF" },
    { id: "photo-formats-nav", label: "Formats", sub: "•••", nav: "photo-formats" },
    { id: "photo-tools-nav", label: "Tools", sub: "✦", nav: "photo-tools" },
    { id: "back", label: "Back", sub: "‹", nav: "category" },
  ],
  -Math.PI / 2
);

// ─── Photo > Formats ────────────────────────────────────────────────────
const PHOTO_FORMATS = alignedSubRing(
  "photo-formats",
  [
    { id: "jpeg", label: "JPEG" },
    { id: "avif", label: "AVIF" },
    { id: "tiff", label: "TIFF" },
    { id: "gif", label: "GIF" },
    { id: "bmp", label: "BMP" },
    { id: "ico", label: "ICO", sub: "multi" },
    { id: "icns", label: "ICNS", sub: "macOS" },
    { id: "back", label: "Back", sub: "‹", nav: "photo" },
  ],
  PHOTO,
  "photo-formats",
  "photo"
);

// ─── Photo > Tools ──────────────────────────────────────────────────────
const PHOTO_TOOLS = alignedSubRing(
  "photo-tools",
  [
    { id: "photo-compress-nav", label: "Compress", sub: "size", nav: "photo-compress" },
    { id: "nobg", label: "Remove", sub: "BG", disabled: !IS_MAC },
    { id: "photo-transform-nav", label: "Transform", sub: "↻", nav: "photo-transform" },
    { id: "square", label: "Square", sub: "1:1" },
    { id: "gif-anim", label: "GIF", sub: "anim" },
    { id: "clean", label: "Clean", sub: "EXIF" },
    { id: "back", label: "Back", sub: "‹", nav: "photo" },
  ],
  PHOTO,
  "photo-tools",
  "photo"
);

// ─── Photo > Tools > Compress (target byte budgets & lossless) ──────────
const PHOTO_COMPRESS = alignedSubRing(
  "photo-compress",
  [
    { id: "cmp-auto", label: "Auto", sub: "smaller" },
    { id: "cmp-1m", label: "1 MB", sub: "max" },
    { id: "cmp-500k", label: "500 KB", sub: "max" },
    { id: "cmp-200k", label: "200 KB", sub: "max" },
    { id: "cmp-100k", label: "100 KB", sub: "max" },
    { id: "cmp-lossless", label: "Lossless", sub: "exact" },
    { id: "back", label: "Back", sub: "‹", nav: "photo-tools" },
  ],
  PHOTO_TOOLS,
  "photo-compress",
  "photo-tools"
);

// ─── Photo > Tools > Transform ──────────────────────────────────────────
const PHOTO_TRANSFORM = alignedSubRing(
  "photo-transform",
  [
    { id: "rot-r", label: "Rotate", sub: "90° ›" },
    { id: "rot-l", label: "Rotate", sub: "‹ 90°" },
    { id: "rot-180", label: "Rotate", sub: "180°" },
    { id: "flip-h", label: "Flip", sub: "horiz" },
    { id: "flip-v", label: "Flip", sub: "vert" },
    { id: "grayscale", label: "B&W", sub: "gray" },
    { id: "back", label: "Back", sub: "‹", nav: "photo-tools" },
  ],
  PHOTO_TOOLS,
  "photo-transform",
  "photo-tools"
);

// ─── Office room ────────────────────────────────────────────────────────
const OFFICE = ring(
  "office",
  [
    { id: "pdf", label: "PDF" },
    { id: "docx", label: "WORD" },
    { id: "docx-ocr", label: "WORD", sub: "OCR" },
    { id: "back", label: "Back", sub: "‹", nav: "category" },
  ],
  -Math.PI / 2
);

// ─── Video room (placeholder — formats coming soon) ─────────────────────
const VIDEO = ring(
  "video",
  [
    { id: "mp4", label: "MP4", disabled: true },
    { id: "webm", label: "WEBM", disabled: true },
    { id: "mov", label: "MOV", disabled: true },
    { id: "avi", label: "AVI", disabled: true },
    { id: "back", label: "Back", sub: "‹", nav: "category" },
  ],
  -Math.PI / 2
);

// ─── Music room (placeholder — formats coming soon) ─────────────────────
const MUSIC = ring(
  "music",
  [
    { id: "mp3", label: "MP3", disabled: true },
    { id: "wav", label: "WAV", disabled: true },
    { id: "flac", label: "FLAC", disabled: true },
    { id: "ogg", label: "OGG", disabled: true },
    { id: "back", label: "Back", sub: "‹", nav: "category" },
  ],
  -Math.PI / 2
);

// ─── Size / quality presets (Option/Alt ring, unchanged) ────────────────
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

export const RINGS: Record<RingId, Ring> = {
  category: CATEGORY,
  photo: PHOTO,
  "photo-formats": PHOTO_FORMATS,
  "photo-tools": PHOTO_TOOLS,
  "photo-compress": PHOTO_COMPRESS,
  "photo-transform": PHOTO_TRANSFORM,
  office: OFFICE,
  video: VIDEO,
  music: MUSIC,
  size: SIZE,
};

// The default ring when opening (drag enter or paste)
export const DEFAULT_RING: RingId = "category";

const CUSTOM_LABELS: Record<string, string> = {
  "docx-ocr": "WORD OCR",
  nobg: "Remove BG",
  "gif-anim": "Animated GIF",
  "cmp-auto": "Auto Compress",
  "cmp-1m": "Max 1 MB",
  "cmp-500k": "Max 500 KB",
  "cmp-200k": "Max 200 KB",
  "cmp-100k": "Max 100 KB",
  "cmp-lossless": "Lossless",
  "rot-r": "Rotate 90° CW",
  "rot-l": "Rotate 90° CCW",
  "rot-180": "Rotate 180°",
  "flip-h": "Flip Horizontal",
  "flip-v": "Flip Vertical",
  grayscale: "Grayscale",
  square: "Square 1:1",
};

export function labelOf(id: string): string | undefined {
  if (CUSTOM_LABELS[id]) return CUSTOM_LABELS[id];
  for (const r of Object.values(RINGS)) {
    const item = r.items.find((i) => i.id === id);
    if (item) return item.label;
  }
}

// Whether a ring is a category (lobby) ring
export function isCategoryRing(id: RingId): boolean {
  return id === "category";
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
