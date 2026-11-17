import { describe, expect, it } from "vitest";
import { MAX_RING_SLOTS, type CatalogEntry } from "../../workshop/workshop";
import { placeChip } from "./chipPlacement";
import type { ChipDrag } from "./types";

const entry = (id: string): CatalogEntry => ({ id, label: id.toUpperCase(), title: id, category: "photo" });
const drag = (id: string, targetSlice: ChipDrag["targetSlice"], fromSlotIndex: number | null = null): ChipDrag => ({
  item: entry(id),
  fromSlotIndex,
  targetSlice,
  x: 0,
  y: 0,
});

describe("placeChip", () => {
  it("does nothing when released away from the ring and the slots", () => {
    expect(placeChip(["a", "b"], drag("x", null))).toBeNull();
  });

  describe("on the core", () => {
    it("adds a new item at the end", () => {
      expect(placeChip(["a", "b"], drag("x", "core"))).toEqual(["a", "b", "x"]);
    });

    it("ignores an item already on the ring", () => {
      expect(placeChip(["a", "b"], drag("a", "core"))).toBeNull();
    });

    it("ignores a full ring", () => {
      const full = Array.from({ length: MAX_RING_SLOTS }, (_, i) => `s${i}`);
      expect(placeChip(full, drag("x", "core"))).toBeNull();
    });
  });

  describe("on a slot", () => {
    it("moves a slot chip to the target position", () => {
      expect(placeChip(["a", "b", "c"], drag("a", 2, 0))).toEqual(["b", "c", "a"]);
      expect(placeChip(["a", "b", "c"], drag("c", 0, 2))).toEqual(["c", "a", "b"]);
    });

    it("replaces the slot with a catalog item", () => {
      expect(placeChip(["a", "b"], drag("x", 0))).toEqual(["x", "b"]);
    });

    it("moves a catalog item that is already on the ring instead of duplicating it", () => {
      expect(placeChip(["a", "b", "c"], drag("a", 2))).toEqual(["b", "c", "a"]);
    });

    it("clamps a target past the last slot (the Back segment) to the last slot", () => {
      expect(placeChip(["a", "b"], drag("x", 5))).toEqual(["a", "x"]);
    });
  });

  it("never mutates the slots it is given", () => {
    const slots = ["a", "b", "c"];
    placeChip(slots, drag("a", 2, 0));
    placeChip(slots, drag("x", "core"));
    expect(slots).toEqual(["a", "b", "c"]);
  });
});
