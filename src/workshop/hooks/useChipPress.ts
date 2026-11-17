import { useEffect, useRef, type PointerEvent as ReactPointerEvent } from "react";
import type { CatalogEntry } from "../workshop";

// Pointer travel (px) before a press on a chip becomes a drag instead of a click
const DRAG_THRESHOLD = 5;

type Options = {
  /** The press moved far enough: hand the chip over to the ring's drag (useChipDrag) */
  onDragStart: (item: CatalogEntry, fromSlotIndex: number | null, clientX: number, clientY: number) => void;
  /** A catalog chip was clicked without moving */
  onClick: (item: CatalogEntry) => void;
};

/** Tells a click on a Workshop chip from the start of a drag */
export function useChipPress({ onDragStart, onClick }: Options) {
  const stopPress = useRef<(() => void) | null>(null);

  useEffect(() => () => stopPress.current?.(), []);

  return (e: ReactPointerEvent, item: CatalogEntry, fromSlotIndex: number | null) => {
    if (e.button !== 0) return;
    const start = { x: e.clientX, y: e.clientY };
    let moved = false;

    const onMove = (ev: PointerEvent) => {
      if (!moved && Math.hypot(ev.clientX - start.x, ev.clientY - start.y) > DRAG_THRESHOLD) {
        moved = true;
        onDragStart(item, fromSlotIndex, ev.clientX, ev.clientY);
      }
    };
    const stop = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      stopPress.current = null;
    };
    const onUp = () => {
      stop();
      // Slot chips are only dragged; their × button removes them
      if (!moved && fromSlotIndex === null) onClick(item);
    };

    stopPress.current?.();
    stopPress.current = stop;
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };
}

export type PressChip = ReturnType<typeof useChipPress>;
