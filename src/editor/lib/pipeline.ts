// The edits and their arithmetic, shared by the live preview and (mirrored line for line)
// the full-resolution save in src-tauri/src/convert/edit.rs: rotate, flip, crop, scale,
// then adjust. Everything is in fractions and levels, never pixels of one resolution,
// so the preview (at most 2048 px) and the saved photo get exactly the same edits.

export type Rotation = 0 | 90 | 180 | 270;

/** Fractions (0..1) of the rotated and flipped photo */
export type Crop = { x: number; y: number; width: number; height: number };

/** -100..100 each, 0 leaves the photo as it is */
export type Adjust = { brightness: number; contrast: number; saturation: number; warmth: number };

export type Edits = {
  /** Clockwise */
  rotation: Rotation;
  /** In the rotated frame, as seen on screen */
  flipH: boolean;
  flipV: boolean;
  /** null keeps the whole photo */
  crop: Crop | null;
  /** 0 < scale <= 1 of the cropped size */
  scale: number;
  adjust: Adjust;
};

export const NO_ADJUST: Adjust = { brightness: 0, contrast: 0, saturation: 0, warmth: 0 };
export const NO_EDITS: Edits = { rotation: 0, flipH: false, flipV: false, crop: null, scale: 1, adjust: NO_ADJUST };
export const FULL_CROP: Crop = { x: 0, y: 0, width: 1, height: 1 };

/** Warmth at ±100 shifts red up and blue down (or the reverse) by this many levels */
const WARMTH_LEVELS = 25;

const clamp = (v: number, min: number, max: number) => Math.min(max, Math.max(min, v));

export const sameEdits = (a: Edits, b: Edits) => JSON.stringify(a) === JSON.stringify(b);
export const isIdentityAdjust = (a: Adjust) =>
  a.brightness === 0 && a.contrast === 0 && a.saturation === 0 && a.warmth === 0;

// ─── Geometry ─────────────────────────────────────────────────────────────

export type Size = { width: number; height: number };

export const rotatedSize = ({ width, height }: Size, rotation: Rotation): Size =>
  rotation === 90 || rotation === 270 ? { width: height, height: width } : { width, height };

/** The crop in whole pixels, always inside the photo and at least 1×1 (crop_pixels in edit.rs) */
export function cropPixels(crop: Crop, width: number, height: number) {
  const frac = (v: number) => (Number.isFinite(v) ? clamp(v, 0, 1) : 0);
  const x = Math.min(Math.round(frac(crop.x) * width), Math.max(0, width - 1));
  const y = Math.min(Math.round(frac(crop.y) * height), Math.max(0, height - 1));
  const w = clamp(Math.round(frac(crop.width) * width), 1, width - x);
  const h = clamp(Math.round(frac(crop.height) * height), 1, height - y);
  return { x, y, width: w, height: h };
}

/** The size the saved photo will have */
export function outputSize(original: Size, edits: Edits): Size {
  const frame = rotatedSize(original, edits.rotation);
  const cropped = edits.crop ? cropPixels(edits.crop, frame.width, frame.height) : frame;
  const side = (s: number) => Math.max(1, Math.round(s * clamp(edits.scale, 0.01, 1)));
  return edits.scale < 1 ? { width: side(cropped.width), height: side(cropped.height) } : cropped;
}

/**
 * Turns the photo a quarter turn, keeping what is on screen: the crop turns with it, and a
 * flip swaps axis (flipping then turning is turning then flipping the other way).
 */
export function turn(edits: Edits, direction: "cw" | "ccw"): Edits {
  const rotation = ((edits.rotation + (direction === "cw" ? 90 : 270)) % 360) as Rotation;
  const c = edits.crop;
  const crop =
    c &&
    (direction === "cw"
      ? { x: 1 - c.y - c.height, y: c.x, width: c.height, height: c.width }
      : { x: c.y, y: 1 - c.x - c.width, width: c.height, height: c.width });
  return { ...edits, rotation, flipH: edits.flipV, flipV: edits.flipH, crop };
}

/** Mirrors what is on screen, the crop with it */
export function flip(edits: Edits, axis: "h" | "v"): Edits {
  const c = edits.crop;
  if (axis === "h") return { ...edits, flipH: !edits.flipH, crop: c && { ...c, x: 1 - c.x - c.width } };
  return { ...edits, flipV: !edits.flipV, crop: c && { ...c, y: 1 - c.y - c.height } };
}

/**
 * The largest crop of `ratio` (width / height, in pixels) centred in a `frame`;
 * null ratio keeps the whole frame.
 */
export function fitAspect(ratio: number | null, frame: Size): Crop {
  if (ratio === null) return FULL_CROP;
  // The ratio in fractions of the frame
  const r = (ratio * frame.height) / frame.width;
  const width = r >= 1 ? 1 : r;
  const height = r >= 1 ? 1 / r : 1;
  return { x: (1 - width) / 2, y: (1 - height) / 2, width, height };
}

export type Handle = "move" | "n" | "s" | "e" | "w" | "nw" | "ne" | "sw" | "se";

/** The crop can never shrink below this fraction of the frame */
export const MIN_CROP = 0.04;

/**
 * The crop after dragging `handle` by (dx, dy) fractions of the frame from `start`.
 * `ratio` (in fractions of the frame, see fitAspect) keeps its shape, anchored at the
 * opposite corner or edge; the crop always stays inside the frame.
 */
export function dragCrop(start: Crop, handle: Handle, dx: number, dy: number, ratio: number | null): Crop {
  if (handle === "move") {
    return {
      ...start,
      x: clamp(start.x + dx, 0, 1 - start.width),
      y: clamp(start.y + dy, 0, 1 - start.height),
    };
  }
  const has = (side: string) => handle.includes(side);
  let left = start.x;
  let top = start.y;
  let right = start.x + start.width;
  let bottom = start.y + start.height;
  if (has("w")) left = clamp(left + dx, 0, right - MIN_CROP);
  if (has("e")) right = clamp(right + dx, left + MIN_CROP, 1);
  if (has("n")) top = clamp(top + dy, 0, bottom - MIN_CROP);
  if (has("s")) bottom = clamp(bottom + dy, top + MIN_CROP, 1);
  if (ratio === null) return { x: left, y: top, width: right - left, height: bottom - top };

  let width = right - left;
  let height = bottom - top;
  const horizontal = has("e") || has("w");
  const vertical = has("n") || has("s");
  if (horizontal && vertical) {
    // A corner: the side that moved most leads
    if (width / height > ratio) height = width / ratio;
    else width = height * ratio;
  } else if (horizontal) height = width / ratio;
  else width = height * ratio;

  // Anchored at the opposite side, or centred on the side's own axis
  const ax = has("w") ? start.x + start.width : has("e") ? start.x : start.x + start.width / 2;
  const ay = has("n") ? start.y + start.height : has("s") ? start.y : start.y + start.height / 2;
  const maxW = has("w") ? ax : has("e") ? 1 - ax : 2 * Math.min(ax, 1 - ax);
  const maxH = has("n") ? ay : has("s") ? 1 - ay : 2 * Math.min(ay, 1 - ay);
  const fit = Math.min(1, maxW / width, maxH / height);
  width *= fit;
  height *= fit;
  return {
    x: has("w") ? ax - width : has("e") ? ax : ax - width / 2,
    y: has("n") ? ay - height : has("s") ? ay : ay - height / 2,
    width,
    height,
  };
}

// ─── Tone (Tone in edit.rs) ───────────────────────────────────────────────

/** Brightness, contrast and warmth per channel: 256 levels each for R, G, B */
function toneTable(a: Adjust): Float64Array {
  const gain = 2 ** (a.brightness / 100);
  const contrast = 1 + a.contrast / 100;
  const warmth = (a.warmth / 100) * WARMTH_LEVELS;
  const shift = [warmth, 0, -warmth];
  const lut = new Float64Array(768);
  for (let channel = 0; channel < 3; channel++) {
    for (let v = 0; v < 256; v++) lut[channel * 256 + v] = (v * gain - 128) * contrast + 128 + shift[channel];
  }
  return lut;
}

const toLevel = (v: number) => ((v < 0 ? 0 : v > 255 ? 255 : v) + 0.5) | 0;

/** Adjusts RGBA pixels in place, then saturation around each pixel's luminance; alpha is kept */
export function applyTone(data: Uint8ClampedArray, a: Adjust) {
  if (isIdentityAdjust(a)) return;
  const lut = toneTable(a);
  const s = 1 + a.saturation / 100;
  for (let i = 0; i < data.length; i += 4) {
    const r = lut[data[i]];
    const g = lut[256 + data[i + 1]];
    const b = lut[512 + data[i + 2]];
    const luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    data[i] = toLevel(luma + (r - luma) * s);
    data[i + 1] = toLevel(luma + (g - luma) * s);
    data[i + 2] = toLevel(luma + (b - luma) * s);
  }
}
