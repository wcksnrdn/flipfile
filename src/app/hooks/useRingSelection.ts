import { useCallback, useRef, useState } from "react";
import { zoneAt, type Point } from "../../rings/dropTarget";
import { DEBUG_DROP, type DropDebugInfo } from "../../debug/DebugOverlay";
import { HOVER_GRACE_MS, SAME_FORMAT } from "../lib/constants";
import type { Picking, ResolveType } from "../lib/types";
import { isMergeable } from "../lib/utils";
import type { RingNavigation } from "./useRingNavigation";
import type { ScaleFactor } from "./useScaleFactor";

type Options = {
  nav: RingNavigation;
  coreCenter: () => Point;
  scale: ScaleFactor;
};

/**
 * The files in hand (dragged in, or pasted and waiting for a click) and what they point
 * at on the ring: the hovered slice, the Option/Alt preset, and how a point resolves to
 * a format. Shared by drag & drop, paste-and-click and the Option/Alt ring.
 */
export function useRingSelection({ nav, coreCenter, scale }: Options) {
  const [dragging, setDragging] = useState(false);
  const [picking, setPicking] = useState<Picking | null>(null);
  const [fileCount, setFileCount] = useState(0);
  const [mergeable, setMergeable] = useState(false);
  const [hovered, setHovered] = useState<string | null>(null);
  // Size/quality preset chosen with Option/Alt for the current drop
  const [preset, setPreset] = useState<string | null>(null);
  const [debugInfo, setDebugInfo] = useState<DropDebugInfo | null>(null);

  // Drag handlers are synchronous, so what they read between renders lives in refs.
  const presetRef = useRef<string | null>(null);
  const handPaths = useRef<string[]>([]);
  const lastHover = useRef<{ id: string; at: number } | null>(null);
  const lastIndex = useRef<number | null>(null);
  const lastPoint = useRef<Point | null>(null);
  /** Whether Option/Alt is held (polled by useAltPresets) */
  const altHeld = useRef(false);

  const getPreset = useCallback(() => presetRef.current, []);
  const getPaths = useCallback(() => handPaths.current, []);
  const isAltHeld = useCallback(() => altHeld.current, []);
  const setAltHeld = useCallback((held: boolean) => (altHeld.current = held), []);

  const choosePreset = (id: string | null) => {
    presetRef.current = id;
    setPreset(id);
  };

  const hold = (paths: string[]) => {
    handPaths.current = paths;
    setFileCount(paths.length);
    setMergeable(isMergeable(paths));
  };

  /** Highlights a slice; a hover recorded this way still counts for a drop just after it */
  const setHover = (id: string | null) => {
    if (id) lastHover.current = { id, at: performance.now() };
    setHovered(id);
  };

  // Maps a point (CSS px) to the selected format, for both drag & drop and paste-and-click.
  // "over"/"move" may swap rings; "drop" may fall back to a hover from the last moment.
  const resolve = (type: ResolveType, point: Point, raw: Point = point): string | null => {
    const center = coreCenter();
    const rings = nav.getRings();
    let detail = zoneAt(point, center, rings[nav.getCurrentRing()]);

    // Entering a navigation segment (category items, "More"/"Back") swaps the ring.
    // Only on entering, so the aligned segment under the cursor right after a swap
    // does not swap straight back.
    const moving = type === "over" || type === "move";
    if (moving && detail.zone.kind === "item" && detail.zone.index !== lastIndex.current) {
      // A sub-ring whose pack isn't here is a slice like the others: dropping gets the pack
      const target = detail.zone.item.pack ? undefined : detail.zone.item.nav;
      if (target) {
        nav.switchRing(target);
        detail = zoneAt(point, center, rings[target]);
      }
    }
    lastIndex.current = detail.zone.kind === "item" ? detail.zone.index : null;
    if (moving) lastPoint.current = point;

    const item = detail.zone.kind === "item" ? detail.zone.item : null;
    // On the Option/Alt ring, pointing at a preset picks it for this drop
    if (item?.preset && (moving || type === "drop" || type === "click")) choosePreset(item.id);
    // Dropping right on a preset converts in the file's own format with that preset
    const id = item?.preset ? SAME_FORMAT : item && !item.disabled && (!item.nav || item.pack) ? item.id : null;

    let result = id;
    if (type === "drop" && !id && detail.zone.kind !== "core") {
      // Never fall back when the user deliberately released over the core.
      const last = lastHover.current;
      if (last && performance.now() - last.at <= HOVER_GRACE_MS) result = last.id;
    }

    if (DEBUG_DROP) {
      const info: DropDebugInfo = {
        type,
        raw,
        dpr: window.devicePixelRatio,
        scaleFactor: scale.get(),
        point,
        center,
        ring: nav.getCurrentRing(),
        detail,
        result: result ?? "—",
      };
      console.log("[drop-debug]", info);
      setDebugInfo(info);
    }
    return result;
  };

  /** Lets go of the files: back to the entry ring with nothing hovered or chosen */
  const closeRing = () => {
    lastHover.current = null;
    lastIndex.current = null;
    lastPoint.current = null;
    altHeld.current = false;
    choosePreset(null);
    nav.resetRing();
    setDragging(false);
    setPicking(null);
    setHovered(null);
  };

  return {
    dragging,
    picking,
    fileCount,
    mergeable,
    hovered,
    preset,
    debugInfo,
    isAltHeld,
    setAltHeld,
    resolve,
    setHover,
    /** Highlights a slice for display only (Workshop previews), without the drop grace */
    setHovered,
    closeRing,
    getPreset,
    getPaths,
    /** Files dragged in from the OS; replaces any pending paste */
    startDrag: (paths: string[]) => {
      closeRing();
      setDragging(true);
      hold(paths);
    },
    /** Files or an image pasted, waiting for a click on a format */
    startPicking: (paths: string[], image: boolean) => {
      nav.resetRing();
      lastIndex.current = null;
      hold(paths);
      setPicking({ paths, image });
    },
    /** Re-evaluates where the cursor already is, e.g. after the ring changed under it */
    reaim: () => {
      lastIndex.current = null;
      setHover(lastPoint.current ? resolve("over", lastPoint.current) : null);
    },
  };
}

export type RingSelection = ReturnType<typeof useRingSelection>;
