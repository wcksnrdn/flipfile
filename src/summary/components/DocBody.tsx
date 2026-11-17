import { invoke } from "@tauri-apps/api/core";
import { explain, type Doc } from "../summary";
import { RetryIcon } from "./icons";
import SummaryView from "./SummaryView";
import Working from "./Working";

type Props = {
  doc: Doc;
  checked: ReadonlySet<number>;
  onCheck: (index: number) => void;
  onAsk: ((question: string) => void) | null;
  onRetry: () => void;
};

/** The active document's body: why it failed, its summary, or the model at work */
export default function DocBody({ doc, checked, onCheck, onAsk, onRetry }: Props) {
  if (doc.stage === "failed") return <Failure doc={doc} onRetry={onRetry} />;
  if (doc.summary && Object.keys(doc.summary).length > 0) {
    return (
      <SummaryView
        summary={doc.summary}
        writing={doc.stage === "writing"}
        checked={checked}
        onCheck={onCheck}
        answers={doc.answers}
        onAsk={onAsk}
        peek={doc.translating ? undefined : doc.peek}
      />
    );
  }
  return <Working doc={doc} />;
}

function Failure({ doc, onRetry }: { doc: Doc; onRetry: () => void }) {
  const info = explain(doc.error?.code ?? "", doc.error?.message ?? "");
  return (
    <div className="sp-failure">
      <div className="sp-failure-mark">!</div>
      <h2>{info.title}</h2>
      <p>{info.text}</p>
      {info.action === "settings" && (
        <button className="sp-button primary" onClick={() => invoke("summary_settings")}>
          Open System Settings
        </button>
      )}
      {info.action === "retry" && (
        <button className="sp-button" onClick={onRetry}>
          <RetryIcon /> Try again
        </button>
      )}
    </div>
  );
}

/** Nothing handed over yet */
export function EmptyPanel() {
  return (
    <div className="sp-empty">
      <div className="sp-orb calm" aria-hidden="true">
        <span />
      </div>
      <h2>Drop a document here</h2>
      <p>
        PDF, Word, PowerPoint, Excel, web pages, e-books, a photo of text, or a recording or video. Apple Intelligence
        explains it right here, on this Mac.
      </p>
    </div>
  );
}
