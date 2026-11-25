import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { RingId } from "../../rings/dropTarget";
import type { PackStatus } from "../../packs/packs";
import { buildWorkspaceRings, type WorkspaceConfig } from "../../workshop/workshop";

/**
 * The rings built for the active workspace and which one is showing. Drag events are
 * synchronous, so the latest rings and current ring are also readable through getters.
 */
export function useRingNavigation(workspace: WorkspaceConfig, aiSupported: boolean, packs: PackStatus[]) {
  // Rebuilt once the backend says whether AI summaries can run here, and when a pack comes or
  // goes: buildWorkspaceRings reads those from workshop.ts (setAiSupport) and packs.ts
  // (setPacks), so `aiSupported` and `packs` are real dependencies the linter can't see
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const built = useMemo(() => buildWorkspaceRings(workspace), [workspace, aiSupported, packs]);
  const ringList = useMemo(() => Object.values(built.rings), [built.rings]);

  // Read by drag handlers between renders; layout effects run before the next event is handled
  const builtRef = useRef(built);
  useLayoutEffect(() => {
    builtRef.current = built;
  }, [built]);

  const [ringId, setRingId] = useState<RingId>(built.entryRing);
  const ringRef = useRef<RingId>(built.entryRing);

  const switchRing = useCallback((id: RingId) => {
    ringRef.current = id;
    setRingId(id);
  }, []);

  /** Back to the ring a drag starts on */
  const resetRing = useCallback(() => switchRing(builtRef.current.entryRing), [switchRing]);
  const getRings = useCallback(() => builtRef.current.rings, []);
  const getEntryRing = useCallback(() => builtRef.current.entryRing, []);
  const getCurrentRing = useCallback(() => ringRef.current, []);

  return { built, ringList, ringId, switchRing, resetRing, getRings, getEntryRing, getCurrentRing };
}

export type RingNavigation = ReturnType<typeof useRingNavigation>;
