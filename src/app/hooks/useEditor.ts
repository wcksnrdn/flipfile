import { useRef, useState } from "react";
import type { Point } from "../../rings/dropTarget";
import { EDITOR_SIZING, loadEditorSize } from "../../editor/editorWindow";
import type { PhotoRequest } from "../../editor/hooks/usePhoto";
import { GLASS_IDLE, sendGlass } from "../lib/glass";
import { usePanelWindow } from "./usePanelWindow";

// Lets the window finish resizing before the glass disc is recentred
const GLASS_RECENTER_DELAY_MS = 80;

/**
 * The Photo Editor (src/editor): a photo dropped on the ring's Edit slice opens it in a
 * card beside the circle, at the size the user last chose, until it is closed.
 */
export function useEditor({ coreCenter }: { coreCenter: () => Point }) {
  const [request, setRequest] = useState<PhotoRequest | null>(null);
  const nextId = useRef(0);
  // The card comes in once the window has grown, and goes before it shrinks
  const panel = usePanelWindow();

  const recenterGlass = () =>
    window.setTimeout(() => sendGlass(coreCenter(), GLASS_IDLE).catch(() => {}), GLASS_RECENTER_DELAY_MS);

  /** Edits `path` (a pasted image's temporary file saves to Downloads) */
  const openPhoto = (path: string, toDownloads: boolean) => {
    nextId.current += 1;
    setRequest({ id: nextId.current, path, toDownloads });
    panel.show(loadEditorSize().size, EDITOR_SIZING.min);
    recenterGlass();
  };

  const close = () => {
    if (request === null) return;
    panel.hide(() => {
      setRequest(null);
      recenterGlass();
    });
  };

  return {
    /** A photo is open (the card may still be on its way in or out) */
    open: request !== null,
    panel: panel.phase,
    request,
    openPhoto,
    close,
    onWindowResized: () => void recenterGlass(),
  };
}

export type Editor = ReturnType<typeof useEditor>;
