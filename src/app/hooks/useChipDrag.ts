import { useEffect, useRef, useState } from "react";
import { RING_OUTER, zoneAt, type Point, type RingId } from "../../rings/dropTarget";
import { getSlotsForRing, type CatalogEntry, type WorkspaceConfig } from "../../workshop/workshop";
import { placeChip } from "../lib/chipPlacement";
import type { ChipDrag } from "../lib/types";
import type { RingNavigation } from "./useRingNavigation";

// How far outside the ring a chip still aims at it
const RING_REACH = 32;

type Options = {
  workspace: WorkspaceConfig;
  ringToEdit: RingId;
  nav: RingNavigation;
  coreCenter: () => Point;
  setHovered: (id: string | null) => void;
  setSlots: (ring: RingId, slots: string[]) => void;
};

/** Dragging a Workshop chip from the catalog or the slot list onto the ring */
export function useChipDrag({ workspace, ringToEdit, nav, coreCenter, setHovered, setSlots }: Options) {
  const [chipDrag, setChipDrag] = useState<ChipDrag | null>(null);
  const stopListening = useRef<(() => void) | null>(null);

  useEffect(() => () => stopListening.current?.(), []);

  const startChipDrag = (item: CatalogEntry, fromSlotIndex: number | null, clientX: number, clientY: number) => {
    // The workspace and ring as they are when the drag starts
    const ring = ringToEdit;
    const slots = getSlotsForRing(workspace, ring);
    nav.switchRing(ring);
    let drag: ChipDrag = { item, fromSlotIndex, x: clientX, y: clientY, targetSlice: null };
    setChipDrag(drag);

    const targetAt = (pt: Point): ChipDrag["targetSlice"] => {
      const center = coreCenter();
      if (Math.hypot(pt.x - center.x, pt.y - center.y) <= RING_OUTER + RING_REACH) {
        const { zone } = zoneAt(pt, center, nav.getRings()[ring]);
        // Past the editable slots (the "Back" segment) counts as the core: add to the ring
        const onSlot = zone.kind === "item" && zone.index < slots.length;
        setHovered(onSlot ? zone.item.id : null);
        return onSlot ? zone.index : "core";
      }
      setHovered(null);
      const el = document.elementFromPoint(pt.x, pt.y);
      const slotChip = el?.closest<HTMLElement>("[data-slot-index]");
      if (slotChip?.dataset.slotIndex !== undefined) return Number(slotChip.dataset.slotIndex);
      if (el?.closest('[data-drop-zone="workbench"]')) return "core";
      return null;
    };

    const onMove = (ev: PointerEvent) => {
      drag = { item, fromSlotIndex, x: ev.clientX, y: ev.clientY, targetSlice: targetAt({ x: ev.clientX, y: ev.clientY }) };
      setChipDrag(drag);
    };

    const onUp = () => {
      stop();
      setHovered(null);
      setChipDrag(null);
      const next = placeChip(slots, drag);
      if (next) setSlots(ring, next);
    };

    const stop = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      stopListening.current = null;
    };

    stopListening.current?.();
    stopListening.current = stop;
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  return { chipDrag, startChipDrag };
}
