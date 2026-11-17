import type { CSSProperties } from "react";
import { cls } from "../../shared/cls";
import { languageName, type Doc } from "../summary";
import { Peek } from "./primitives";

type Progress = { status: string; detail: string; progress?: number };

/** What the model is doing with the document right now, in words and (when known) as a fraction */
function progressOf(doc: Doc): Progress {
  if (doc.translating) {
    return {
      status: `Translating into ${languageName(doc.translating)}`,
      detail: "Names, numbers and dates stay exactly as they are",
    };
  }
  if (doc.stage === "writing") {
    return {
      status: "Writing the summary",
      detail: doc.parts ? `From notes on all ${doc.parts} parts` : "Picking out what matters",
    };
  }
  if (doc.stage === "thinking" && doc.parts) {
    return {
      status: `Taking notes · part ${doc.part ?? 1} of ${doc.parts}`,
      detail:
        doc.kept !== undefined && doc.kept < 0.98
          ? `A long one: reading its ${Math.max(1, Math.round(doc.kept * 100))}% most telling passages`
          : "A long one: reading it part by part",
      progress: ((doc.part ?? 1) - 1) / doc.parts,
    };
  }
  if (doc.listening) {
    return {
      status: doc.page ? `Listening · ${doc.page}%` : "Listening",
      detail: "Writing down what is said, right here on this Mac",
      progress: doc.page ? doc.page / 100 : undefined,
    };
  }
  if (doc.readPages && doc.readPages > 1) {
    return {
      status: `Reading page ${Math.min((doc.page ?? 0) + 1, doc.readPages)} of ${doc.readPages}`,
      detail: doc.ocr ? "Recognizing the scanned text" : "Getting the text out",
      progress: (doc.page ?? 0) / doc.readPages,
    };
  }
  return { status: "Reading", detail: "Apple Intelligence reads it right here on this Mac" };
}

// The latest bullet points of the notes, newest last
const latestNotes = (doc: Doc) =>
  doc.notes
    .flatMap((note, part) =>
      note
        .split("\n")
        .map((line) => line.replace(/^\s*[-•*]\s*/, "").trim())
        .filter(Boolean)
        .map((text, i) => ({ key: `${part}-${i}`, text }))
    )
    .slice(doc.peek ? -2 : -4);

/** Before any of the summary exists: the model reading, with its notes as they come */
export default function Working({ doc }: { doc: Doc }) {
  const { status, detail, progress } = progressOf(doc);
  const lines = latestNotes(doc);

  return (
    <div className={cls("sp-working", doc.peek && "peeking")}>
      <div className="sp-orb" aria-hidden="true">
        <span />
      </div>
      <div className="sp-status" key={status}>
        {status}
      </div>
      <div className="sp-detail">{detail}</div>
      {progress !== undefined && (
        <div className="sp-bar">
          <span style={{ width: `${Math.max(4, progress * 100)}%` }} />
        </div>
      )}
      {lines.length > 0 && (
        <ul className="sp-notes">
          {lines.map((line, i) => (
            <li key={line.key} style={{ "--age": lines.length - 1 - i } as CSSProperties}>
              {line.text}
            </li>
          ))}
        </ul>
      )}
      {doc.peek && !doc.translating && <Peek sentences={doc.peek} />}
    </div>
  );
}
