import { afterEach, describe, expect, it } from "vitest";
import { buildWorkspaceRings, packFor, type WorkspaceConfig } from "../workshop/workshop";
import { packNeeded, packSub, setPacks, sizeLabel, type PackStatus } from "./packs";

const pack = (overrides: Partial<PackStatus> = {}): PackStatus => ({
  id: "upscale",
  name: "Upscale",
  targets: ["up-2x", "up-4x"],
  state: "missing",
  size: 145_000_000,
  download: 145_000_000,
  done: 0,
  total: 0,
  removable: false,
  ...overrides,
});
const transcribe = pack({ id: "transcribe", name: "Transcribe", targets: ["music-transcribe", "tr-srt"], size: 191_000_000, download: 191_000_000 });
const allRooms: WorkspaceConfig = { id: "all-rooms", name: "All Rooms", mode: "rooms", customSlots: [], roomSlots: {} };

afterEach(() => setPacks([]));

describe("packs", () => {
  it("a format or sub-ring needs its pack until the pack is usable", () => {
    const packs = [pack(), transcribe];
    expect(packNeeded("up-2x", undefined, packs)?.id).toBe("upscale");
    expect(packNeeded("music-transcribe-nav", "music-transcribe", packs)?.id).toBe("transcribe");
    expect(packNeeded("png", undefined, packs)).toBeUndefined();
    expect(packNeeded("up-2x", undefined, [pack({ state: "installed" })])).toBeUndefined();
    // A newer version out: the one here still works
    expect(packNeeded("up-2x", undefined, [pack({ state: "update" })])).toBeUndefined();
    expect(packNeeded("up-2x", undefined, [pack({ state: "downloading" })])?.id).toBe("upscale");
  });

  it("a slice shows the download's size, then its progress", () => {
    expect(packSub(pack())).toBe("↓ 145 MB");
    expect(packSub(pack({ download: 12_400_000 }))).toBe("↓ 12 MB");
    expect(packSub(pack({ state: "downloading" }), { id: "upscale", done: 50, total: 200 })).toBe("25%");
    expect(packSub(pack({ state: "restart" }))).toBe("restart");
    expect(sizeLabel(1_250_000_000)).toBe("1.3 GB");
  });

  it("the rings keep a missing feature's slices, marked, and their sub-rings closed", () => {
    setPacks([pack(), transcribe]);
    const { rings } = buildWorkspaceRings(allRooms);
    const up = rings["photo-upscale"].items.find((i) => i.id === "up-2x");
    expect(up).toMatchObject({ pack: "upscale", sub: "↓ 145 MB" });
    expect(rings["photo-upscale"].items.find((i) => i.id === "up-text-2x")?.pack).toBeUndefined();
    const nav = rings.music.items.find((i) => i.nav === "music-transcribe");
    expect(nav).toMatchObject({ pack: "transcribe", sub: "↓ 191 MB" });
    expect(packFor("music-transcribe-nav")?.id).toBe("transcribe");
    expect(packFor("tr-srt")?.id).toBe("transcribe");

    setPacks([pack({ state: "installed" }), { ...transcribe, state: "installed" }]);
    const after = buildWorkspaceRings(allRooms).rings;
    expect(after["photo-upscale"].items.find((i) => i.id === "up-2x")).toMatchObject({ pack: undefined, sub: "2×" });
  });
});
