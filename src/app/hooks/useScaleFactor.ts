import { useCallback, useEffect, useMemo, useRef } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { subscription } from "../lib/utils";

/**
 * The window's scale factor, kept at hand because drag events are synchronous.
 * It changes when the window moves to a monitor with different scaling.
 */
export function useScaleFactor() {
  const scale = useRef(window.devicePixelRatio || 1);

  const refresh = useCallback(() => {
    getCurrentWindow()
      .scaleFactor()
      .then((f) => (scale.current = f))
      .catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
    return subscription(getCurrentWindow().onScaleChanged(({ payload }) => (scale.current = payload.scaleFactor)));
  }, [refresh]);

  return useMemo(() => ({ get: () => scale.current, refresh }), [refresh]);
}

export type ScaleFactor = ReturnType<typeof useScaleFactor>;
