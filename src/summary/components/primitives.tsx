import type { CSSProperties, ReactNode } from "react";
import { cls } from "../../shared/cls";
import { figures } from "../summary";
import { QuoteIcon } from "./icons";

/** Text with its amounts, percentages and dates highlighted */
export function Rich({ text }: { text: string }) {
  return (
    <>
      {figures(text).map((piece, i) =>
        piece.figure ? (
          <mark key={i} className="sp-figure">
            {piece.text}
          </mark>
        ) : (
          piece.text
        )
      )}
    </>
  );
}

/** The blinking caret at the end of text that is still being written */
export const Caret = ({ on }: { on: boolean }) => (on ? <span className="sp-caret" aria-hidden="true" /> : null);

export function Skeleton({ lines, className }: { lines: number; className?: string }) {
  return (
    <div className={cls("sp-skeleton", className)} aria-hidden="true">
      {Array.from({ length: lines }, (_, i) => (
        <span key={i} style={{ width: i === lines - 1 && lines > 1 ? "62%" : "100%" }} />
      ))}
    </div>
  );
}

export function Section({ title, count, children }: { title: string; count?: number; children: ReactNode }) {
  return (
    <section className="sp-section">
      <h3 className="sp-section-title">
        {title}
        {count ? <span className="sp-count">{count}</span> : null}
      </h3>
      {children}
    </section>
  );
}

/** The document's own most telling sentences, there the moment it is read, while the
 *  model is still on its way to a summary */
export function Peek({ sentences }: { sentences: string[] }) {
  return (
    <div className="sp-peek">
      <div className="sp-peek-label">
        <QuoteIcon />
        Quick look · from the document
      </div>
      <ul>
        {sentences.map((sentence, i) => (
          <li key={i} style={{ "--i": i } as CSSProperties}>
            <Rich text={sentence} />
          </li>
        ))}
      </ul>
    </div>
  );
}
