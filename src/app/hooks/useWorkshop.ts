import { useRef, useState } from "react";
import type { Point, RingId } from "../../rings/dropTarget";
import { useTauriEvent } from "../../shared/tauri";
import { editableRing } from "../../workshop/workshop";
import { applyWorkshopWindow } from "../../workshop/workshopWindow";
import { GLASS_IDLE, GLASS_OPEN, sendGlass } from "../lib/glass";
import type { RingNavigation } from "./useRingNavigation";
import type { Workspaces } from "./useWorkspaces";

// Lets the window finish resizing before the glass disc is recentred
const GLASS_RECENTER_DELAY_MS = 80;

type Options = {
  workspaces: Workspaces;
  nav: RingNavigation;
  coreCenter: () => Point;
  clearHover: () => void;
};

/**
 * The Ring Workshop: opening and closing it (tray, Cmd/Ctrl+, , right-click, the badge),
 * which ring is being edited, and the panel's actions, which also keep the ring shown in
 * step with what is being edited.
 */
export function useWorkshop({ workspaces, nav, coreCenter, clearHover }: Options) {
  const [open, setOpen] = useState(false);
  const openRef = useRef(false);
  const [editingRing, setEditingRing] = useState<RingId>("photo");
  const active = workspaces.active;
  /** The ring the Workshop edits for the active workspace */
  const ringToEdit = editableRing(active.mode, editingRing);

  const toggle = (nextOpen?: boolean) => {
    const target = nextOpen ?? !openRef.current;
    openRef.current = target;
    setOpen(target);
    applyWorkshopWindow(target);

    if (target) {
      nav.switchRing(ringToEdit);
    } else {
      nav.resetRing();
      clearHover();
    }
    window.setTimeout(
      () => sendGlass(coreCenter(), target ? GLASS_OPEN : GLASS_IDLE).catch(() => {}),
      GLASS_RECENTER_DELAY_MS
    );
  };

  /** Edits a room ring and shows it */
  const editRing = (ring: RingId) => {
    setEditingRing(ring);
    nav.switchRing(ring);
  };

  // "Ring Workshop..." from the tray menu
  useTauriEvent("toggle-workshop", () => toggle());

  const panelHandlers = {
    onSelectWorkspace: (id: string) => {
      const target = workspaces.state.workspaces.find((w) => w.id === id);
      workspaces.select(id);
      if (target) nav.switchRing(editableRing(target.mode, editingRing));
    },
    onCreateWorkspace: (name: string) => {
      workspaces.create(name);
      nav.switchRing("custom");
    },
    onDeleteWorkspace: workspaces.remove,
    onSetMode: (mode: "rooms" | "direct") => {
      workspaces.setMode(mode);
      nav.switchRing(editableRing(mode, editingRing));
    },
    onSelectEditingRing: editRing,
    onUpdateSlots: workspaces.setSlots,
    onResetRing: workspaces.resetRing,
  };

  return { open, editingRing, ringToEdit, toggle, editRing, panelHandlers };
}

export type Workshop = ReturnType<typeof useWorkshop>;
