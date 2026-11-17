import { useRef, type MouseEvent } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { DRAG_THRESHOLD } from "../lib/constants";

type Options = {
  onClick: () => void;
  onContextMenu: () => void;
};

/**
 * The core both moves the window (press + drag) and acts on a click.
 * data-tauri-drag-region would start a window drag on every press and swallow clicks.
 */
export function useCoreGestures({ onClick, onContextMenu }: Options) {
  const press = useRef<{ x: number; y: number } | null>(null);

  return {
    onMouseDown: (e: MouseEvent) => {
      if (e.button === 0) press.current = { x: e.clientX, y: e.clientY };
    },
    onMouseMove: (e: MouseEvent) => {
      const p = press.current;
      if (p && Math.hypot(e.clientX - p.x, e.clientY - p.y) > DRAG_THRESHOLD) {
        press.current = null;
        getCurrentWindow().startDragging();
      }
    },
    onMouseUp: () => {
      if (!press.current) return;
      press.current = null;
      onClick();
    },
    onMouseLeave: () => {
      press.current = null;
    },
    onContextMenu: (e: MouseEvent) => {
      e.preventDefault();
      onContextMenu();
    },
  };
}
