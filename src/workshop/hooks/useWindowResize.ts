import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { DEFAULT_PRESET, resizeWorkshopWindow, WINDOW_PRESETS } from "../workshopWindow";

// A second nudge after resizing, for platforms that settle the size a moment later
const SETTLE_MS = 60;

/**
 * Sizing the borderless Workshop window: the S / M / L presets and the corner grip.
 * `onResized` lets the circle recentre its glass disc each time.
 */
export function useWindowResize(onResized: () => void) {
  const [preset, setPreset] = useState<string>(DEFAULT_PRESET);
  const stopGrip = useRef<(() => void) | null>(null);

  useEffect(() => () => stopGrip.current?.(), []);

  const resize = (width: number, height: number) => {
    resizeWorkshopWindow(width, height).then(onResized).catch(() => {});
    window.setTimeout(onResized, SETTLE_MS);
  };

  const applyPreset = (id: string) => {
    const p = WINDOW_PRESETS.find((p) => p.id === id);
    if (!p) return;
    setPreset(id);
    resize(p.size.width, p.size.height);
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
      raf = requestAnimationFrame(() => resize(start.width + (ev.screenX - start.x), start.height + (ev.screenY - start.y)));
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

  return { preset, applyPreset, onGripDown };
}
