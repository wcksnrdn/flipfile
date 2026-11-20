// Drawing the live preview: the tone is applied to the preview's pixels (applyTone),
// then the canvas turns, flips and crops it, the same way edit.rs does at full size.

import { applyTone, cropPixels, rotatedSize, type Adjust, type Edits } from "./pipeline";

/** A canvas holding `pixels`, with `adjust` applied */
export function tonedCanvas(pixels: ImageData, adjust: Adjust): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = pixels.width;
  canvas.height = pixels.height;
  const copy = new ImageData(new Uint8ClampedArray(pixels.data), pixels.width, pixels.height);
  applyTone(copy.data, adjust);
  canvas.getContext("2d")?.putImageData(copy, 0, 0);
  return canvas;
}

/** The size in pixels of what drawEdited shows */
export function shownSize(source: { width: number; height: number }, edits: Edits, wholeFrame: boolean) {
  const frame = rotatedSize(source, edits.rotation);
  return !wholeFrame && edits.crop ? cropPixels(edits.crop, frame.width, frame.height) : { x: 0, y: 0, ...frame };
}

/**
 * Draws `source` turned and flipped onto `canvas`: the whole frame (while the crop is
 * being chosen on top of it), or only what the crop keeps. With `maxWidth` (the saved
 * photo's width) it is drawn no sharper than it will be saved, so a photo resized very
 * small looks as soft on screen as in the file.
 */
export function drawEdited(
  canvas: HTMLCanvasElement,
  source: HTMLCanvasElement,
  edits: Edits,
  wholeFrame: boolean,
  maxWidth = Infinity
) {
  const frame = rotatedSize(source, edits.rotation);
  const shown = shownSize(source, edits, wholeFrame);
  const k = Math.min(1, maxWidth / shown.width);
  canvas.width = Math.max(1, Math.round(shown.width * k));
  canvas.height = Math.max(1, Math.round(shown.height * k));
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.imageSmoothingQuality = "high";
  ctx.scale(k, k);
  ctx.translate(frame.width / 2 - shown.x, frame.height / 2 - shown.y);
  ctx.scale(edits.flipH ? -1 : 1, edits.flipV ? -1 : 1);
  ctx.rotate((edits.rotation * Math.PI) / 180);
  ctx.drawImage(source, -source.width / 2, -source.height / 2);
}

/** `pixels` scaled down to fit in `max` px, for the looks' thumbnails */
export function thumbnailPixels(pixels: ImageData, max: number): ImageData {
  const ratio = Math.min(1, max / Math.max(pixels.width, pixels.height));
  const full = tonedCanvas(pixels, { brightness: 0, contrast: 0, saturation: 0, warmth: 0 });
  const small = document.createElement("canvas");
  small.width = Math.max(1, Math.round(pixels.width * ratio));
  small.height = Math.max(1, Math.round(pixels.height * ratio));
  const ctx = small.getContext("2d", { willReadFrequently: true });
  if (!ctx) return pixels;
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(full, 0, 0, small.width, small.height);
  return ctx.getImageData(0, 0, small.width, small.height);
}
