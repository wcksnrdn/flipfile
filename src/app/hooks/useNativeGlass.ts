import { useEffect, useRef, useState } from "react";
import type { Point } from "../../rings/dropTarget";
import { easeInOut, easeOutBack, GLASS_IDLE, GLASS_OPEN, sendGlass } from "../lib/glass";

const GROW_MS = 420;
const SHRINK_MS = 260;

type Options = {
  coreCenter: () => Point;
  /** The ring is showing: the disc grows to cover it */
  open: boolean;
  /** The Workshop is open: the window can be resized, so the disc follows its center */
  workshopOpen: boolean;
};

/** The native glass disc behind the ring (macOS only); returns whether it is in use */
export function useNativeGlass({ coreCenter, open, workshopOpen }: Options) {
  const [nativeGlass, setNativeGlass] = useState(false);
  const size = useRef(GLASS_IDLE);

  // Native glass is only available on macOS; elsewhere the command returns false
  useEffect(() => {
    sendGlass(coreCenter(), GLASS_IDLE)
      .then(setNativeGlass)
      .catch(() => setNativeGlass(false));
  }, [coreCenter]);

  // Grow / shrink the glass disc in step with the ring animation
  useEffect(() => {
    if (!nativeGlass) return;
    const from = size.current;
    const to = open ? GLASS_OPEN : GLASS_IDLE;
    if (from === to) return;
    const duration = open ? GROW_MS : SHRINK_MS;
    const ease = open ? easeOutBack : easeInOut;
    const start = performance.now();
    let frame = 0;

    const step = (now: number) => {
      const t = Math.min(1, (now - start) / duration);
      size.current = from + (to - from) * ease(t);
      sendGlass(coreCenter(), size.current);
      if (t < 1) frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
    return () => cancelAnimationFrame(frame);
  }, [open, nativeGlass, coreCenter]);

  // Keep the disc aligned with the ring center whenever the Workshop window is resized
  useEffect(() => {
    if (!workshopOpen) return;
    const onResize = () => sendGlass(coreCenter(), GLASS_OPEN).catch(() => {});
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [workshopOpen, coreCenter]);

  return nativeGlass;
}
