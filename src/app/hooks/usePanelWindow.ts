import { useEffect, useRef, useState } from "react";
import { applyPanelWindow, type Size } from "../../workshop/workshopWindow";

/**
 * A panel beside the circle (the Workshop, the Photo Editor) and the window around it:
 *
 *   hidden -> (the window grows) -> shown -> leaving -> (the window shrinks) -> hidden
 *
 * The panel only appears once the window has its room, so its entrance (panel-in in
 * workshop/styles/motion.css) plays in full instead of behind a window still growing;
 * and it leaves (panel-out) before the window shrinks around it.
 */
export type PanelPhase = "hidden" | "shown" | "leaving";

/** As long as panel-out */
const LEAVE_MS = 260;

export function usePanelWindow() {
  const [phase, setPhaseState] = useState<PanelPhase>("hidden");
  const phaseRef = useRef<PanelPhase>("hidden");
  // Bumped by every show and hide: a slower step from an earlier one does nothing
  const turn = useRef(0);
  const leaveTimer = useRef(0);

  const setPhase = (next: PanelPhase) => {
    phaseRef.current = next;
    setPhaseState(next);
  };

  useEffect(() => () => window.clearTimeout(leaveTimer.current), []);

  /** Grows the window to `size`, then shows the panel */
  const show = (size: Size, min: Size) => {
    const mine = ++turn.current;
    window.clearTimeout(leaveTimer.current);
    // Still leaving: the window has not shrunk yet, the panel just comes back
    if (phaseRef.current !== "hidden") {
      setPhase("shown");
      return;
    }
    applyPanelWindow(true, size, min).then(() => {
      if (turn.current === mine) setPhase("shown");
    });
  };

  /**
   * Lets the panel leave, then shrinks the window back to the circle (unless `keepWindow`:
   * another panel takes the window over); `done` runs after
   */
  const hide = (done?: () => void, keepWindow = false) => {
    const mine = ++turn.current;
    window.clearTimeout(leaveTimer.current);
    const finish = () => {
      if (turn.current !== mine) return;
      setPhase("hidden");
      if (!keepWindow) applyPanelWindow(false);
      done?.();
    };
    // Handing the window over: gone at once, so the two panels never share it
    if (phaseRef.current !== "hidden" && !keepWindow) {
      setPhase("leaving");
      leaveTimer.current = window.setTimeout(finish, LEAVE_MS);
    } else finish();
  };

  return { phase, show, hide };
}
