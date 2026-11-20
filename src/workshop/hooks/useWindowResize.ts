import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { DEFAULT_PRESET, resizePanelWindow, WORKSHOP_SIZING, type PanelSizing, type Size } from "../workshopWindow";

// A second nudge after resizing, for platforms that settle the size a moment later
const SETTLE_MS = 60;

type Options = {
  sizing?: PanelSizing;
  /** The preset shown as chosen at first ("" for a size of the user's own) */
  initialPreset?: string;
  /** The window took a size the user chose: a preset's id, or "" from the grip */
  onSized?: (size: Size, preset: string) => void;
};

/**
 * Sizing a borderless panel window (the Workshop, the Photo Editor): its S / M / L
 * presets and the corner grip. `onResized` lets the circle recentre its glass disc each time.
 */
export function useWindowResize(
  onResized: () => void,
  { sizing = WORKSHOP_SIZING, initialPreset = DEFAULT_PRESET, onSized }: Options = {}
) {
  const [preset, setPreset] = useState<string>(initialPreset);
  const stopGrip = useRef<(() => void) | null>(null);

  useEffect(() => () => stopGrip.current?.(), []);

  const resize = (width: number, height: number, presetId: string) => {
    resizePanelWindow(width, height, sizing)
      .then((size) => {
        onResized();
        onSized?.(size, presetId);
      })
      .catch(() => {});
    window.setTimeout(onResized, SETTLE_MS);
  };

  const applyPreset = (id: string) => {
    const p = sizing.presets.find((p) => p.id === id);
    if (!p) return;
    setPreset(id);
    resize(p.size.width, p.size.height, id);
  };

  // Drag the bottom-right corner grip to resize the window smoothly, one frame at a time
  const onGripDown = (e: ReactPointerEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const start = { x: e.screenX, y: e.screenY, width: window.innerWidth, height: window.innerHeight };
    setPreset("");

    let raf = 0;
    const onMove = (ev: PointerEvent) => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() =>
        resize(start.width + (ev.screenX - start.x), start.height + (ev.screenY - start.y), "")
      );
    };
    const stop = () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      stopGrip.current = null;
    };
    const onUp = () => {
      stop();
      onResized();
    };

    stopGrip.current?.();
    stopGrip.current = stop;
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  return { preset, presets: sizing.presets, applyPreset, onGripDown };
}
