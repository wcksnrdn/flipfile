import { useCallback, useRef } from "react";
import type { Point } from "../../rings/dropTarget";

/** The core element, and its center: the origin of every ring hit-test and the glass disc */
export function useCoreAnchor() {
  const coreRef = useRef<HTMLDivElement>(null);
  const coreCenter = useCallback((): Point => {
    const rect = coreRef.current?.getBoundingClientRect();
    return rect
      ? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }
      : { x: window.innerWidth / 2, y: window.innerHeight / 2 };
  }, []);
  return { coreRef, coreCenter };
}
