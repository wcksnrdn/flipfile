import { useCallback, useMemo, useState } from "react";
import type { RingId } from "../../rings/dropTarget";
import {
  defaultSlots,
  getSlotsForRing,
  loadWorkspacesState,
  saveWorkspacesState,
  type WorkspaceConfig,
  type WorkspacesState,
} from "../../workshop/workshop";

/** The saved workspaces (ring layouts) and the operations the Workshop performs on them */
export function useWorkspaces() {
  const [state, setState] = useState<WorkspacesState>(loadWorkspacesState);

  const active = useMemo(
    () => state.workspaces.find((w) => w.id === state.activeId) ?? state.workspaces[0],
    [state]
  );

  // Every change is saved as it is made (not from an effect, which would also write the
  // untouched defaults on launch and pin them for users who never customised anything)
  const change = useCallback((updater: (prev: WorkspacesState) => WorkspacesState) => {
    setState((prev) => {
      const next = updater(prev);
      saveWorkspacesState(next);
      return next;
    });
  }, []);

  const updateActive = (fn: (ws: WorkspaceConfig) => WorkspaceConfig) =>
    change((prev) => ({ ...prev, workspaces: prev.workspaces.map((ws) => (ws.id === prev.activeId ? fn(ws) : ws)) }));

  const setSlots = (ring: RingId, slots: string[]) =>
    updateActive((ws) =>
      ring === "custom" ? { ...ws, customSlots: slots } : { ...ws, roomSlots: { ...ws.roomSlots, [ring]: slots } }
    );

  return {
    state,
    active,
    setSlots,
    resetRing: (ring: RingId) => setSlots(ring, defaultSlots(ring)),
    setMode: (mode: WorkspaceConfig["mode"]) => updateActive((ws) => ({ ...ws, mode })),
    select: (id: string) => change((prev) => ({ ...prev, activeId: id })),
    /** Adds a direct workspace seeded with the current custom ring, and makes it active */
    create: (name: string) => {
      const id = `ws-${Date.now()}`;
      const customSlots = [...getSlotsForRing(active, "custom")];
      change((prev) => ({
        activeId: id,
        workspaces: [...prev.workspaces, { id, name, mode: "direct", customSlots, roomSlots: {} }],
      }));
    },
    remove: (id: string) =>
      change((prev) => {
        const workspaces = prev.workspaces.filter((w) => w.id !== id);
        return { activeId: prev.activeId === id ? workspaces[0].id : prev.activeId, workspaces };
      }),
  };
}

export type Workspaces = ReturnType<typeof useWorkspaces>;
