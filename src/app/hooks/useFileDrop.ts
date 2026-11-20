import { useEffect, useEffectEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { PhysicalPosition } from "@tauri-apps/api/dpi";
import { getCurrentWebview, type DragDropEvent } from "@tauri-apps/api/webview";
import { toCssPoint } from "../../rings/dropTarget";
import { DEBUG_DROP } from "../../debug/DebugOverlay";
import { isDocument } from "../../summary/summary";
import { hasAiSupport } from "../../workshop/workshop";
import type { Execute } from "../lib/types";
import { subscription } from "../../shared/tauri";
import type { RingSelection } from "./useRingSelection";
import type { ScaleFactor } from "./useScaleFactor";

type Options = {
  selection: RingSelection;
  scale: ScaleFactor;
  execute: Execute;
  /** Off while the Photo Editor is open: a photo dropped there opens in it instead */
  enabled: boolean;
};

/** Files dragged in from the OS: the ring follows the cursor, and a drop runs the format under it */
export function useFileDrop({ selection, scale, execute, enabled }: Options) {
  const at = (position: PhysicalPosition) => ({
    point: toCssPoint(position, scale.get()),
    raw: { x: position.x, y: position.y },
  });

  const onDragDrop = useEffectEvent((p: DragDropEvent) => {
    if (!enabled) return;
    switch (p.type) {
      case "enter": {
        scale.refresh();
        selection.startDrag(p.paths);
        // It may be headed for Summary: the model loads while the user aims
        if (hasAiSupport() && p.paths.some(isDocument)) invoke("summary_warm").catch(() => {});
        const { point, raw } = at(p.position);
        selection.setHover(selection.resolve(p.type, point, raw));
        break;
      }
      case "over": {
        const { point, raw } = at(p.position);
        selection.setHover(selection.resolve(p.type, point, raw));
        break;
      }
      case "leave":
        if (DEBUG_DROP) console.log("[drop-debug] leave");
        selection.closeRing();
        break;
      case "drop": {
        const { point, raw } = at(p.position);
        const format = selection.resolve(p.type, point, raw);
        const preset = selection.getPreset();
        selection.closeRing();
        if (format) execute(p.paths, format, false, preset);
        break;
      }
    }
  });

  useEffect(() => subscription(getCurrentWebview().onDragDropEvent((event) => onDragDrop(event.payload))), []);
}
