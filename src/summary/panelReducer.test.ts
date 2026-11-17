import { describe, expect, it } from "vitest";
import { panelReducer } from "./panelReducer";
import { EMPTY, type Doc, type PanelState } from "./summary";

const started = (files: string[]) => panelReducer(EMPTY, { type: "start", files });

const withDocs = (...docs: Partial<Doc>[]): PanelState => {
  const state = started(docs.map((_, i) => `doc${i}.pdf`));
  return { ...state, docs: state.docs.map((d, i) => ({ ...d, ...docs[i] })) };
};

describe("panelReducer", () => {
  it("starts a run with one waiting document per file", () => {
    const state = started(["a.pdf", "b.docx"]);
    expect(state.running).toBe(true);
    expect(state.active).toBe(0);
    expect(state.docs.map((d) => [d.name, d.stage])).toEqual([
      ["a.pdf", "waiting"],
      ["b.docx", "waiting"],
    ]);
  });

  it("follows the user's pick of a tab", () => {
    const state = panelReducer(started(["a", "b"]), { type: "pick", file: 1 });
    expect(state).toMatchObject({ active: 1, pinned: true });
  });

  it("adds a pending answer to the asked document only", () => {
    const state = panelReducer(withDocs({ stage: "done" }, { stage: "done" }), { type: "ask", file: 1, question: "Who?" });
    expect(state.docs[0].answers).toEqual([]);
    expect(state.docs[1].answers).toEqual([{ question: "Who?", text: "", done: false }]);
  });

  describe("closing the panel", () => {
    it("leaves unfinished documents stopped, and finished ones as they are", () => {
      const state = panelReducer(withDocs({ stage: "done" }, { stage: "writing" }, { stage: "failed" }), {
        type: "closed",
      });
      expect(state.running).toBe(false);
      expect(state.docs.map((d) => d.stage)).toEqual(["done", "failed", "failed"]);
      expect(state.docs[1].error).toEqual({ code: "cancelled", message: "" });
    });

    it("stops answers still being written", () => {
      const answers = [
        { question: "Q1", text: "A1", done: true },
        { question: "Q2", text: "", done: false },
      ];
      const [doc] = panelReducer(withDocs({ stage: "done", answers }), { type: "closed" }).docs;
      expect(doc.answers[0]).toEqual(answers[0]);
      expect(doc.answers[1]).toMatchObject({ done: true, error: "Stopped when the panel was closed" });
    });
  });

  it("passes the model's events on to the summary reducer", () => {
    const state = panelReducer(started(["a.pdf"]), { type: "finished" });
    expect(state.running).toBe(false);
  });

  it("ignores events for a document it doesn't have", () => {
    const state = started(["a.pdf"]);
    expect(panelReducer(state, { type: "writing", file: 7 })).toBe(state);
  });
});
