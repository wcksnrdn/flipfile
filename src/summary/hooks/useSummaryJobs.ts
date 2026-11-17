import { useEffect, useEffectEvent, useReducer, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { subscription } from "../../shared/tauri";
import { panelReducer } from "../panelReducer";
import { EMPTY, type SummaryEvent } from "../summary";

const NONE_CHECKED: ReadonlySet<number> = new Set();

// Keeps only the entries for files other than `file` (or none, when everything starts over)
const freshFor = (event: Extract<SummaryEvent, { type: "start" | "restart" }>) =>
  <T,>(all: Record<number, T>): Record<number, T> =>
    event.type === "start" ? {} : Object.fromEntries(Object.entries(all).filter(([file]) => Number(file) !== event.file));

/**
 * The documents being summarized: the model's streams (only the latest run's is shown),
 * questions about a finished summary, and what the user saved or ticked off per document.
 */
export function useSummaryJobs(onError: (message: string) => void) {
  const [state, dispatch] = useReducer(panelReducer, EMPTY);
  const [saved, setSaved] = useState<Record<number, string>>({});
  const [checked, setChecked] = useState<Record<number, Set<number>>>({});
  const runs = useRef(0);
  const owner = useRef(0);

  // Each run streams on its own channel; only the latest run's events are shown.
  // A question belongs to the panel as it is now: its channel takes over at once.
  const follow = (start: (onEvent: Channel<SummaryEvent>) => Promise<unknown>, own = false) => {
    const id = ++runs.current;
    if (own) owner.current = id;
    const onEvent = new Channel<SummaryEvent>();
    onEvent.onmessage = (event) => {
      if (event.type === "start" || event.type === "restart") {
        owner.current = id;
        // What was saved or ticked belonged to the summary that is now being replaced
        const fresh = freshFor(event);
        setSaved(fresh);
        setChecked(fresh);
      }
      if (owner.current === id) dispatch(event);
    };
    start(onEvent).catch((e) => onError(String(e)));
  };

  // The job the circle handed over, now and whenever another one arrives
  const pickUp = useEffectEvent(() => follow((onEvent) => invoke("summary_run", { onEvent })));
  useEffect(() => {
    pickUp();
    return subscription(listen("summary-job", () => pickUp()));
  }, []);

  const active = state.active;
  const doc = state.docs[active];
  const busy = state.running;
  const finished = doc?.stage === "done";
  const asking = !!doc?.answers.some((a) => !a.done);
  const canAsk = finished && !busy && !asking;

  return {
    state,
    doc,
    busy,
    finished,
    asking,
    canAsk,
    savedPath: saved[active] as string | undefined,
    checked: checked[active] ?? NONE_CHECKED,
    pick: (file: number) => dispatch({ type: "pick", file }),
    redo: (language: string | null) =>
      follow((onEvent) => invoke("summary_redo", { file: active, language, onEvent })),
    ask: (question: string) => {
      const q = question.trim();
      if (!q || !canAsk) return;
      dispatch({ type: "ask", file: active, question: q });
      follow((onEvent) => invoke("summary_ask", { file: active, question: q, onEvent }), true);
    },
    toggleCheck: (index: number) =>
      setChecked((all) => {
        const set = new Set(all[active] ?? []);
        if (set.has(index)) set.delete(index);
        else set.add(index);
        return { ...all, [active]: set };
      }),
    markSaved: (file: number, path: string) => setSaved((s) => ({ ...s, [file]: path })),
    /** Closing stops the model and hides the panel */
    close: () => {
      dispatch({ type: "closed" });
      invoke("summary_close").catch(() => {});
    },
  };
}
