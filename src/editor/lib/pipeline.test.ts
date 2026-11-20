import { describe, expect, it } from "vitest";
import { freshHistory, historyReducer, type History } from "./history";
import { LOOKS, lookOf } from "./looks";
import {
  FULL_CROP,
  MIN_CROP,
  NO_ADJUST,
  NO_EDITS,
  applyTone,
  cropPixels,
  dragCrop,
  fitAspect,
  flip,
  outputSize,
  turn,
  type Crop,
  type Edits,
} from "./pipeline";

const pixel = (rgba: number[], adjust: Partial<typeof NO_ADJUST>) => {
  const data = new Uint8ClampedArray(rgba);
  applyTone(data, { ...NO_ADJUST, ...adjust });
  return [...data];
};

const close = (a: Crop, b: Crop) => {
  for (const k of ["x", "y", "width", "height"] as const) expect(a[k]).toBeCloseTo(b[k], 9);
};

describe("applyTone", () => {
  // The same numbers as adjustments_match_the_preview in edit.rs
  it("matches the backend", () => {
    expect(pixel([200, 100, 50, 128], { saturation: -100 })).toEqual([118, 118, 118, 128]);
    expect(pixel([200, 100, 50, 255], { brightness: 100 })).toEqual([255, 200, 100, 255]);
    expect(pixel([200, 100, 50, 255], { contrast: 50, warmth: 40 })).toEqual([246, 86, 1, 255]);
  });

  it("leaves the photo alone without adjustments", () => {
    expect(pixel([1, 2, 3, 4], {})).toEqual([1, 2, 3, 4]);
  });
});

describe("crop geometry", () => {
  it("stays inside the photo, like crop_pixels in edit.rs", () => {
    expect(cropPixels({ x: 0.9, y: 0.9, width: 0.5, height: 0.5 }, 100, 50)).toEqual({ x: 90, y: 45, width: 10, height: 5 });
    expect(cropPixels({ x: NaN, y: -1, width: 0, height: 2 }, 100, 50)).toEqual({ x: 0, y: 0, width: 1, height: 50 });
  });

  it("gives the saved size after rotating, cropping and scaling", () => {
    const original = { width: 4000, height: 3000 };
    expect(outputSize(original, NO_EDITS)).toEqual(original);
    expect(outputSize(original, { ...NO_EDITS, rotation: 90 })).toEqual({ width: 3000, height: 4000 });
    const crop = { x: 0.25, y: 0.5, width: 0.5, height: 0.5 };
    expect(outputSize(original, { ...NO_EDITS, crop, scale: 0.5 })).toEqual({ width: 1000, height: 750 });
  });

  it("fits a ratio in the middle of the frame", () => {
    const frame = { width: 4000, height: 3000 };
    close(fitAspect(1, frame), { x: 0.125, y: 0, width: 0.75, height: 1 });
    close(fitAspect(16 / 9, frame), { x: 0, y: 0.125, width: 1, height: 0.75 });
    expect(fitAspect(null, frame)).toEqual(FULL_CROP);
  });
});

describe("turn and flip", () => {
  const crop = { x: 0.1, y: 0.2, width: 0.3, height: 0.4 };
  const edits: Edits = { ...NO_EDITS, crop };

  it("turns the crop with the photo", () => {
    const cw = turn(edits, "cw");
    expect(cw.rotation).toBe(90);
    close(cw.crop!, { x: 0.4, y: 0.1, width: 0.4, height: 0.3 });
    // Four turns either way bring everything back
    let e = edits;
    for (let i = 0; i < 4; i++) e = turn(e, "ccw");
    expect(e.rotation).toBe(0);
    close(e.crop!, crop);
    close(turn(turn(edits, "cw"), "ccw").crop!, crop);
  });

  it("swaps a flip's axis when turning, keeping what is on screen", () => {
    const flipped = flip(NO_EDITS, "h");
    expect(turn(flipped, "cw")).toMatchObject({ rotation: 90, flipH: false, flipV: true });
  });

  it("mirrors the crop", () => {
    close(flip(edits, "h").crop!, { ...crop, x: 0.6 });
    close(flip(edits, "v").crop!, { ...crop, y: 0.4 });
  });
});

describe("dragCrop", () => {
  const start = { x: 0.2, y: 0.2, width: 0.5, height: 0.5 };

  it("moves within the frame", () => {
    close(dragCrop(start, "move", 0.1, -0.1, null), { ...start, x: 0.3, y: 0.1 });
    close(dragCrop(start, "move", 1, 1, null), { ...start, x: 0.5, y: 0.5 });
  });

  it("resizes from a corner, never past the frame or below the minimum", () => {
    close(dragCrop(start, "se", 0.1, 0.2, null), { x: 0.2, y: 0.2, width: 0.6, height: 0.7 });
    close(dragCrop(start, "nw", -1, -1, null), { x: 0, y: 0, width: 0.7, height: 0.7 });
    expect(dragCrop(start, "e", -1, 0, null).width).toBeCloseTo(MIN_CROP, 9);
  });

  it("keeps a locked ratio anchored at the opposite corner", () => {
    const c = dragCrop(start, "se", 0.2, 0, 1);
    close(c, { x: 0.2, y: 0.2, width: 0.7, height: 0.7 });
    // Too big to fit: shrinks to the frame, still square
    const big = dragCrop(start, "se", 0.5, 0.5, 1);
    close(big, { x: 0.2, y: 0.2, width: 0.8, height: 0.8 });
  });

  it("keeps a locked ratio centred when dragging an edge", () => {
    const c = dragCrop(start, "e", 0.1, 0, 2);
    close(c, { x: 0.2, y: 0.3, width: 0.6, height: 0.3 });
  });
});

describe("history", () => {
  const brighter = (h: History, by: number): Edits => ({ ...h.present, adjust: { ...h.present.adjust, brightness: by } });

  it("makes one undo step of a slider drag", () => {
    let h = freshHistory();
    for (const v of [5, 10, 15]) h = historyReducer(h, { type: "set", edits: brighter(h, v), commit: false });
    h = historyReducer(h, { type: "commit" });
    expect(h.past).toHaveLength(1);
    h = historyReducer(h, { type: "undo" });
    expect(h.present).toEqual(NO_EDITS);
    h = historyReducer(h, { type: "redo" });
    expect(h.present.adjust.brightness).toBe(15);
  });

  it("commits an unfinished change before undoing it", () => {
    let h = freshHistory();
    h = historyReducer(h, { type: "set", edits: brighter(h, 20), commit: false });
    h = historyReducer(h, { type: "undo" });
    expect(h.present).toEqual(NO_EDITS);
    expect(h.future[0].adjust.brightness).toBe(20);
  });

  it("drops the redo steps after a new change", () => {
    let h = freshHistory();
    h = historyReducer(h, { type: "set", edits: brighter(h, 20), commit: true });
    h = historyReducer(h, { type: "undo" });
    h = historyReducer(h, { type: "set", edits: flip(h.present, "h"), commit: true });
    expect(h.future).toEqual([]);
    expect(historyReducer(h, { type: "redo" })).toBe(h);
  });

  it("patches what is on screen", () => {
    let h = historyReducer(freshHistory(), { type: "set", edits: flip(NO_EDITS, "h"), commit: true });
    h = historyReducer(h, { type: "patch", patch: { crop: { x: 0, y: 0, width: 0.5, height: 0.5 } }, commit: true });
    expect(h.present).toMatchObject({ flipH: true, crop: { width: 0.5 } });
    expect(h.past).toHaveLength(2);
  });

  it("ignores a commit that changed nothing", () => {
    const h = freshHistory();
    expect(historyReducer(h, { type: "set", edits: NO_EDITS, commit: true }).past).toHaveLength(0);
  });
});

describe("looks", () => {
  it("recognizes its own adjustments", () => {
    for (const look of LOOKS) expect(lookOf(look.adjust)).toBe(look);
    expect(lookOf({ ...NO_ADJUST, brightness: 1 })).toBeUndefined();
  });
});
