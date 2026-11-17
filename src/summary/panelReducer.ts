import { reduce, type Doc, type PanelState, type SummaryEvent } from "./summary";

/** What changes the panel: the model's stream, plus what the user does in the panel itself */
export type PanelAction =
  | SummaryEvent
  | { type: "pick"; file: number }
  | { type: "closed" }
  | { type: "ask"; file: number; question: string };

export function panelReducer(state: PanelState, action: PanelAction): PanelState {
  if (action.type === "pick") return { ...state, active: action.file, pinned: true };
  if (action.type === "ask") {
    const docs = state.docs.map((d, i) =>
      i === action.file ? { ...d, answers: [...d.answers, { question: action.question, text: "", done: false }] } : d
    );
    return { ...state, docs };
  }
  if (action.type === "closed") {
    // Closing stops the model: what was still being worked on is left as stopped
    const docs = state.docs.map((d): Doc => {
      const answers = d.answers.map((a) => (a.done ? a : { ...a, done: true, error: "Stopped when the panel was closed" }));
      return d.stage === "done" || d.stage === "failed"
        ? { ...d, answers }
        : { ...d, answers, stage: "failed", error: { code: "cancelled", message: "" } };
    });
    return { ...state, docs, running: false };
  }
  return reduce(state, action);
}
