import type { RingId } from "../rings/dropTarget";
import { CATALOG, MAX_RING_SLOTS, MIN_RING_SLOTS, defaultSlots, type CatalogCategory, type CatalogEntry } from "./workshop";

// Pure edits of a ring's slot list, as the Workshop makes them. Each returns the new
// list, or null when the edit would take the ring outside MIN/MAX_RING_SLOTS.

/** Clicking a catalog chip: adds it, or removes it if it is already on the ring */
export function toggleSlot(slots: string[], id: string): string[] | null {
  if (slots.includes(id)) return slots.length > MIN_RING_SLOTS ? slots.filter((s) => s !== id) : null;
  return slots.length < MAX_RING_SLOTS ? [...slots, id] : null;
}

export function removeSlot(slots: string[], index: number): string[] | null {
  if (slots.length <= MIN_RING_SLOTS) return null;
  return slots.filter((_, i) => i !== index);
}

export function isDefaultLayout(ring: RingId, slots: string[]) {
  const defaults = defaultSlots(ring);
  return slots.length === defaults.length && slots.every((id, i) => id === defaults[i]);
}

/** The catalog shown for a ring: never the ring's own sub-ring link, then by category and search */
export function filterCatalog(ring: RingId, category: "all" | CatalogCategory, query: string): CatalogEntry[] {
  const q = query.trim().toLowerCase();
  return CATALOG.filter((item) => {
    if (item.nav === ring) return false;
    if (category !== "all" && item.category !== category) return false;
    if (!q) return true;
    return [item.label, item.title, item.id, item.sub ?? ""].some((text) => text.toLowerCase().includes(q));
  });
}
