import { MAX_RING_SLOTS } from "../../workshop/WorkshopPanel";
import type { ChipDrag } from "./types";

/**
 * The ring's slots after a Workshop chip is released, or null when nothing changes.
 * Dropped on the core it is added (if there is room); on a slot it moves there when it
 * came from the ring, and otherwise replaces that slot or moves the existing copy to it.
 */
export function placeChip(slots: string[], { item, fromSlotIndex, targetSlice }: ChipDrag): string[] | null {
  if (targetSlice === null) return null;
  const next = [...slots];

  if (targetSlice === "core") {
    if (next.includes(item.id) || next.length >= MAX_RING_SLOTS) return null;
    next.push(item.id);
    return next;
  }

  const targetIdx = Math.min(targetSlice, next.length - 1);
  if (fromSlotIndex !== null) {
    // Reorder an existing slot
    const [moved] = next.splice(fromSlotIndex, 1);
    next.splice(targetIdx, 0, moved);
    return next;
  }

  // Replace, or move the catalog item already on the ring into targetIdx
  const existingIdx = next.indexOf(item.id);
  if (existingIdx >= 0 && existingIdx !== targetIdx) {
    const [moved] = next.splice(existingIdx, 1);
    next.splice(targetIdx, 0, moved);
  } else {
    next[targetIdx] = item.id;
  }
  return next;
}
