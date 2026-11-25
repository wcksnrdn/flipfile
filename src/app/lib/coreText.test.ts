import { describe, expect, it } from "vitest";
import { RINGS } from "../../rings/dropTarget";
import { AI_SUMMARY, PHOTO_EDITOR, type CatalogEntry, type WorkspaceConfig } from "../../workshop/workshop";
import { ALT_KEY, ALT_SUPPORTED, ON_DEVICE } from "./constants";
import { coreText, type CoreTextInput } from "./coreText";

const allRooms: WorkspaceConfig = { id: "all-rooms", name: "All Rooms", mode: "rooms", customSlots: [], roomSlots: {} };

const input = (overrides: Partial<CoreTextInput> = {}): CoreTextInput => ({
  open: false,
  dragging: false,
  picking: null,
  workshopOpen: false,
  chipDrag: null,
  ringId: "category",
  rings: RINGS,
  hovered: null,
  preset: null,
  estimates: {},
  fileCount: 0,
  mergeable: false,
  settingsSummary: null,
  activeWorkspace: allRooms,
  status: null,
  editing: null,
  ...overrides,
});

// Files dragged in, aiming at `hovered` on `ringId`
const aiming = (overrides: Partial<CoreTextInput>) => input({ open: true, dragging: true, fileCount: 1, ...overrides });

describe("coreText", () => {
  describe("idle", () => {
    it("invites a drop, with the Workshop badge", () => {
      expect(coreText(input())).toEqual({ title: "Convert", hint: "Drop files here", badge: "Customize" });
    });

    it("names a custom workspace, unless the tray defaults are changed", () => {
      const ws = { ...allRooms, id: "ws-1", name: "Design" };
      expect(coreText(input({ activeWorkspace: ws })).badge).toBe("Design");
      expect(coreText(input({ activeWorkspace: ws, settingsSummary: "1080p" })).badge).toBe("1080p");
    });

    it("shows the last status", () => {
      const t = coreText(input({ status: { title: "✓ Done", hint: "1 file → PNG", tone: "ok" } }));
      expect(t).toMatchObject({ title: "✓ Done", hint: "1 file → PNG" });
    });
  });

  describe("a feature that isn't downloaded", () => {
    const upscale = {
      id: "upscale",
      name: "Upscale",
      targets: ["up-2x"],
      state: "missing" as const,
      size: 145_000_000,
      download: 145_000_000,
      done: 0,
      total: 0,
      removable: false,
    };

    it("says what dropping there gets, and its size", () => {
      const t = coreText(aiming({ ringId: "photo-upscale", hovered: "up-2x", neededPack: { pack: upscale } }));
      expect(t).toEqual({ title: "Upscale Photo 2×", hint: "Release to get Upscale · 145 MB", badge: "Not downloaded yet" });
    });

    it("counts up while it downloads", () => {
      const pack = { ...upscale, state: "downloading" as const };
      const t = coreText(aiming({ ringId: "photo-upscale", hovered: "up-2x", neededPack: { pack, progress: { id: "upscale", done: 1, total: 4 } } }));
      expect(t.hint).toBe("Downloading Upscale · 25%");
    });
  });

  describe("aiming files at the ring", () => {
    it("asks for a category in the lobby", () => {
      expect(coreText(aiming({}))).toMatchObject({ title: "Convert", hint: "Pick a category" });
      expect(coreText(aiming({ hovered: "cat-photo" })).hint).toBe("Pick a category");
    });

    it("lets an action placed in the lobby speak for itself", () => {
      expect(coreText(aiming({ hovered: AI_SUMMARY }))).toMatchObject({
        title: "AI Summary",
        hint: "Release to explain it with AI",
        badge: ON_DEVICE,
      });
    });

    it("names the room when nothing is hovered", () => {
      expect(coreText(aiming({ ringId: "photo" }))).toMatchObject({ title: "Photo", hint: "Point to a format" });
    });

    it("says what releasing on a format does", () => {
      expect(coreText(aiming({ ringId: "photo", hovered: "png" }))).toMatchObject({ title: "PNG", hint: "Release to convert" });
    });

    it("says Click instead of Release for a paste", () => {
      const t = coreText(aiming({ dragging: false, picking: { paths: ["a"], image: false }, ringId: "photo", hovered: "png" }));
      expect(t.hint).toBe("Click to convert");
    });

    it.each([
      ["clean", "Release to strip metadata"],
      [PHOTO_EDITOR, "Release to open in Photo Editor"],
      ["up-4x", "Release to enlarge with AI"],
      ["tr-srt", "Release to make subtitles with AI"],
      ["tr-txt", "Release to write down what is said"],
      ["ico", "Release for multi-size .ico"],
      ["cmp-500k", "Release to compress file size"],
      ["flip-h", "Release to transform photo"],
      ["grayscale", "Release to transform photo"],
      ["vid-720p", "Release to resize video resolution"],
      ["pptx", "Release to make PowerPoint slides"],
      ["pdf", "Release to convert"],
    ])("hints %s as %j", (hovered, hint) => {
      expect(coreText(aiming({ ringId: "photo-tools", hovered })).hint).toBe(hint);
    });

    it("says several images merge into one PDF or presentation", () => {
      const many = { ringId: "photo" as const, fileCount: 3, mergeable: true };
      expect(coreText(aiming({ ...many, hovered: "pdf" })).hint).toBe("Release to merge into 1 PDF");
      expect(coreText(aiming({ ...many, hovered: "pptx" })).hint).toBe("Release to combine into 1 presentation");
      expect(coreText(aiming({ ...many, mergeable: false, hovered: "pdf" })).hint).toBe("Release to convert");
    });

    it("badges the chosen size preset, or how to choose one", () => {
      expect(coreText(aiming({ ringId: "photo", preset: "half" })).badge).toBe("Size: 50%");
      expect(coreText(aiming({ ringId: "photo" })).badge).toBe(ALT_SUPPORTED ? `Hold ${ALT_KEY} to resize` : null);
    });
  });

  describe("the Option/Alt ring", () => {
    it("explains itself until a preset is chosen", () => {
      expect(coreText(aiming({ ringId: "size" }))).toEqual({
        title: "Compress or resize",
        hint: "Point to a level or a size",
        badge: null,
      });
      expect(coreText(aiming({ ringId: "size", preset: "half" }))).toMatchObject({
        title: "50%",
        hint: `Drop: same format, or release ${ALT_KEY}`,
      });
    });

    it("shows a compress level's estimate", () => {
      const estimate = { token: 1, id: "cmp-light", before: 1_000_000, after: 250_000, note: "Looks the same", approximate: true };
      expect(coreText(aiming({ ringId: "size", hovered: "cmp-light", estimates: { "cmp-light": estimate } }))).toEqual({
        title: "1.0 MB → ≈ 250 KB",
        hint: "−75% · Looks the same",
        badge: "Compress: Light",
      });
    });

    it("waits for the estimate, or says the file is already compact", () => {
      expect(coreText(aiming({ ringId: "size", hovered: "cmp-strong" }))).toMatchObject({
        title: "Strong",
        hint: "Working out the size…",
      });
      const flat = { token: 1, id: "cmp-light", before: 500, after: 500, note: "", approximate: false };
      expect(coreText(aiming({ ringId: "size", hovered: "cmp-light", estimates: { "cmp-light": flat } }))).toMatchObject({
        title: "500 B",
        hint: "Already compact",
      });
    });
  });

  describe("the Workshop", () => {
    const chip: CatalogEntry = { id: "webp", label: "WEBP", title: "WebP", category: "photo" };
    const workshop = (overrides: Partial<CoreTextInput>) => input({ open: true, workshopOpen: true, ringId: "photo", ...overrides });

    it("names the ring being edited", () => {
      expect(coreText(workshop({}))).toEqual({ title: "Photo", hint: "Drop on a slice", badge: "Workshop" });
      const direct = { ...allRooms, id: "ws-1", name: "Mine", mode: "direct" as const };
      expect(coreText(workshop({ activeWorkspace: direct })).title).toBe("Mine");
    });

    it("says where a dragged chip will go", () => {
      const at = (targetSlice: number | "core" | null) =>
        coreText(workshop({ chipDrag: { item: chip, fromSlotIndex: null, x: 0, y: 0, targetSlice } }));
      expect(at("core")).toMatchObject({ title: "WEBP", hint: "+ Add to ring" });
      expect(at(2).hint).toBe("Replace slot #3");
      expect(at(null).hint).toBe("Drop on a slice");
    });

    it("previews a hovered slice", () => {
      expect(coreText(workshop({ hovered: "png" }))).toMatchObject({ title: "PNG", hint: "Drop to replace" });
    });
  });

  describe("the Photo Editor", () => {
    it("names the photo being edited, without the Workshop badge", () => {
      expect(coreText(input({ editing: "sunset.jpg" }))).toEqual({ title: "Editing", hint: "sunset.jpg", badge: null });
    });

    it("shows that it was saved", () => {
      const status = { title: "Saved", hint: "sunset_edited.jpg", tone: "ok" as const, outputs: ["/a/sunset_edited.jpg"] };
      expect(coreText(input({ editing: "sunset.jpg", status }))).toEqual({ title: "Saved", hint: "sunset_edited.jpg", badge: null });
    });
  });
});
