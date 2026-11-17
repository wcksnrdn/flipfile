import type { MouseEvent, RefObject } from "react";
import { CORE_SIZE, RING_OUTER, zoneAt, type Point } from "../../rings/dropTarget";
import type { WorkspaceConfig } from "../../workshop/workshop";
import type { Execute } from "../lib/types";
import type { RingNavigation } from "./useRingNavigation";
import type { RingSelection } from "./useRingSelection";
import type { Workshop } from "./useWorkshop";

// How far outside the ring the Workshop still previews and navigates it
const RING_REACH = 18;

type Options = {
  coreRef: RefObject<HTMLDivElement | null>;
  coreCenter: () => Point;
  nav: RingNavigation;
  selection: RingSelection;
  workshop: Workshop;
  workspace: WorkspaceConfig;
  chipDragging: boolean;
  execute: Execute;
};

/**
 * The mouse over the window. While choosing a format for a paste it plays the role of the
 * dragged file; in the Ring Workshop it previews slices and clicks into sub-rings.
 */
export function useRingPointer({ coreRef, coreCenter, nav, selection, workshop, workspace, chipDragging, execute }: Options) {
  const { picking } = selection;

  // The ring item under a Workshop pointer, if it is between innerRadius and the ring's reach
  const workshopItemAt = (pt: Point, innerRadius: number) => {
    const center = coreCenter();
    const dist = Math.hypot(pt.x - center.x, pt.y - center.y);
    if (dist <= innerRadius || dist > RING_OUTER + RING_REACH) return null;
    const { zone } = zoneAt(pt, center, nav.getRings()[nav.getCurrentRing()]);
    return zone.kind === "item" ? zone.item : null;
  };

  const onMouseMove = (e: MouseEvent) => {
    const pt = { x: e.clientX, y: e.clientY };
    if (picking) {
      selection.setHover(selection.resolve("move", pt));
    } else if (workshop.open && !chipDragging) {
      selection.setHovered(workshopItemAt(pt, 0)?.id ?? null);
    }
  };

  const onClick = (e: MouseEvent) => {
    const pt = { x: e.clientX, y: e.clientY };
    if (workshop.open && !picking) {
      const target = workshopItemAt(pt, CORE_SIZE / 2)?.nav;
      if (target && workspace.mode === "rooms") workshop.editRing(target);
      return;
    }
    if (!picking || coreRef.current?.contains(e.target as Node)) return;
    const format = selection.resolve("click", pt);
    if (!format) return;
    const preset = selection.getPreset();
    selection.closeRing();
    execute(picking.paths, format, picking.image, preset);
  };

  return { onMouseMove, onClick };
}
