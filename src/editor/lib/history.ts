// Undo / redo of the edits. A slider changes the edits live on every step but is one
// undo step: `set` without commit while it moves, `commit` once it is let go.

import { NO_EDITS, sameEdits, type Edits } from "./pipeline";

export type History = {
  past: Edits[];
  /** What is on screen */
  present: Edits;
  /** The last undo step: `present` until a live change is committed */
  committed: Edits;
  future: Edits[];
};

export type HistoryAction =
  | { type: "set"; edits: Edits; commit: boolean }
  /** Changes some edits of what is on screen (a drag that outlives the render it began in) */
  | { type: "patch"; patch: Partial<Edits>; commit: boolean }
  | { type: "commit" }
  | { type: "undo" }
  | { type: "redo" }
  | { type: "reset" };

/** Undo steps kept; the oldest are forgotten */
const MAX_STEPS = 100;

export const freshHistory = (): History => ({ past: [], present: NO_EDITS, committed: NO_EDITS, future: [] });

function commit(h: History): History {
  if (sameEdits(h.present, h.committed)) return h;
  return { past: [...h.past, h.committed].slice(-MAX_STEPS), present: h.present, committed: h.present, future: [] };
}

export function historyReducer(h: History, action: HistoryAction): History {
  switch (action.type) {
    case "set": {
      const next = { ...h, present: action.edits };
      return action.commit ? commit(next) : next;
    }
    case "patch": {
      const next = { ...h, present: { ...h.present, ...action.patch } };
      return action.commit ? commit(next) : next;
    }
    case "commit":
      return commit(h);
    case "undo": {
      const settled = commit(h);
      const previous = settled.past[settled.past.length - 1];
      if (!previous) return settled;
      return { past: settled.past.slice(0, -1), present: previous, committed: previous, future: [settled.present, ...settled.future] };
    }
    case "redo": {
      // A change not committed yet is a new step, which drops what could be redone
      const settled = commit(h);
      const [next, ...rest] = settled.future;
      if (!next) return settled;
      return { past: [...settled.past, settled.committed], present: next, committed: next, future: rest };
    }
    // Another photo was opened
    case "reset":
      return freshHistory();
  }
}
