import { useEffect, useMemo, useRef, useState } from "react";
import type { Status } from "../lib/types";

export type StatusActions = {
  /** Shows a status (or clears it), optionally returning to idle after `clearAfterMs` */
  show: (next: Status | null, clearAfterMs?: number) => void;
  /** Updates the current status in place, leaving any pending clear as it is */
  update: (fn: (s: Status | null) => Status | null) => void;
  /** Keeps the current status on screen: drops the pending clear */
  keep: () => void;
};

export type StatusController = StatusActions & { status: Status | null };

/** The circle's status line, and the timer that returns it to idle */
export function useStatus(): StatusController {
  const [status, setStatus] = useState<Status | null>(null);
  const clearTimer = useRef<number | undefined>(undefined);

  const actions = useMemo<StatusActions>(() => {
    const keep = () => window.clearTimeout(clearTimer.current);
    return {
      keep,
      update: setStatus,
      show: (next, clearAfterMs) => {
        keep();
        setStatus(next);
        if (clearAfterMs !== undefined) clearTimer.current = window.setTimeout(() => setStatus(null), clearAfterMs);
      },
    };
  }, []);

  useEffect(() => actions.keep, [actions]);

  return { status, ...actions };
}
