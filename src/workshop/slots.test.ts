import { describe, expect, it } from "vitest";
import { filterCatalog, isDefaultLayout, removeSlot, toggleSlot } from "./slots";
import { CATALOG, MAX_RING_SLOTS, MIN_RING_SLOTS, defaultSlots } from "./workshop";

const slotsOf = (n: number) => Array.from({ length: n }, (_, i) => `s${i}`);

describe("toggleSlot", () => {
  it("adds an item that is not on the ring", () => {
    expect(toggleSlot(["a", "b", "c"], "x")).toEqual(["a", "b", "c", "x"]);
  });

  it("removes an item that is on the ring", () => {
    expect(toggleSlot(["a", "b", "c"], "b")).toEqual(["a", "c"]);
  });

  it("keeps the ring at its minimum size", () => {
    expect(toggleSlot(slotsOf(MIN_RING_SLOTS), "s0")).toBeNull();
  });

  it("keeps the ring at its maximum size", () => {
    expect(toggleSlot(slotsOf(MAX_RING_SLOTS), "x")).toBeNull();
  });
});

describe("removeSlot", () => {
  it("removes the slot at the index", () => {
    expect(removeSlot(["a", "b", "c"], 1)).toEqual(["a", "c"]);
  });

  it("keeps the ring at its minimum size", () => {
    expect(removeSlot(slotsOf(MIN_RING_SLOTS), 0)).toBeNull();
  });
});

describe("isDefaultLayout", () => {
  it("is true for the ring's defaults, in order", () => {
    expect(isDefaultLayout("photo", defaultSlots("photo"))).toBe(true);
  });

  it("is false once reordered, shortened or extended", () => {
    const defaults = defaultSlots("photo");
    expect(isDefaultLayout("photo", [...defaults].reverse())).toBe(false);
    expect(isDefaultLayout("photo", defaults.slice(1))).toBe(false);
    expect(isDefaultLayout("photo", [...defaults, "x"])).toBe(false);
  });
});

describe("filterCatalog", () => {
  it("never offers a ring the link to itself", () => {
    const ids = filterCatalog("photo-tools", "all", "").map((i) => i.id);
    expect(ids).not.toContain("photo-tools-nav");
    expect(filterCatalog("photo", "all", "").map((i) => i.id)).toContain("photo-tools-nav");
  });

  it("filters by category", () => {
    const items = filterCatalog("custom", "music", "");
    expect(items.length).toBeGreaterThan(0);
    expect(items.every((i) => i.category === "music")).toBe(true);
  });

  it("searches label, title, id and sub, ignoring case and spaces around the query", () => {
    const ids = filterCatalog("custom", "all", "  WEBP ").map((i) => i.id);
    expect(ids).toContain("webp");
    expect(filterCatalog("custom", "all", "zzz-no-such-format")).toEqual([]);
  });

  it("shows the whole catalog without a query or category (minus the ring's own link)", () => {
    expect(filterCatalog("custom", "all", "")).toHaveLength(CATALOG.filter((i) => i.nav !== "custom").length);
  });
});
