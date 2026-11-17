import { type FormEvent, type MouseEvent, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { cls } from "../../shared/cls";
import { badgeOf, languageName, metaOf, otherLanguage, readingMinutes, type Doc } from "../summary";
import type { Flash } from "../hooks/useDocumentExport";
import {
  CheckIcon,
  CloseIcon,
  CopyIcon,
  FolderIcon,
  LockIcon,
  RetryIcon,
  SaveIcon,
  SendIcon,
  Sparkle,
  TranslateIcon,
} from "./icons";

// The panel's frame around the document: header, tabs, the question box, the saved
// notice and the footer.

// Drag the card by its header (buttons excepted)
const onHeadDown = (e: MouseEvent) => {
  if (e.button === 0 && !(e.target as HTMLElement).closest("button")) getCurrentWindow().startDragging();
};

export function PanelHeader(props: {
  busy: boolean;
  finished: boolean;
  flash: Flash | null;
  onCopy: () => void;
  onSave: () => void;
  onClose: () => void;
}) {
  const { busy, finished, flash, onCopy, onSave, onClose } = props;
  return (
    <header className="sp-head" onMouseDown={onHeadDown}>
      <div className="sp-brand">
        <Sparkle className={busy ? "live" : ""} />
        <div>
          <div className="sp-brand-name">Summary</div>
          <div className="sp-brand-sub">Apple Intelligence</div>
        </div>
      </div>
      <div className="sp-head-actions">
        <button className="sp-icon-button" title="Copy as text (⌘C)" disabled={!finished} onClick={onCopy}>
          {flash === "copied" ? <CheckIcon /> : <CopyIcon />}
        </button>
        <button className="sp-icon-button" title="Save next to the file (⌘S)" disabled={!finished} onClick={onSave}>
          {flash === "saved" ? <CheckIcon /> : <SaveIcon />}
        </button>
        <button className="sp-icon-button" title="Close (Esc)" onClick={onClose}>
          <CloseIcon />
        </button>
      </div>
    </header>
  );
}

/** One tab per document when several were dropped at once */
export function DocTabs({ docs, active, onPick }: { docs: Doc[]; active: number; onPick: (file: number) => void }) {
  if (docs.length <= 1) return null;
  return (
    <nav className="sp-tabs">
      {docs.map((d, i) => (
        <button
          key={i}
          className={cls("sp-tab", i === active && "active", d.stage)}
          onClick={() => onPick(i)}
          title={d.name}
        >
          <span className="sp-tab-dot" />
          {d.name}
        </button>
      ))}
    </nav>
  );
}

export function FileCard({ doc }: { doc: Doc }) {
  const badge = badgeOf(doc.name);
  const meta = metaOf(doc);
  return (
    <div className="sp-file">
      <div className={`sp-badge ${badge.tone}`}>
        <span>{badge.label}</span>
      </div>
      <div className="sp-file-text">
        <div className="sp-file-name" title={doc.name}>
          {doc.name}
        </div>
        <div className="sp-file-meta">{meta || "Opening…"}</div>
      </div>
    </div>
  );
}

const MAX_QUESTION_LENGTH = 500;

export function AskBox(props: {
  draft: string;
  onDraft: (draft: string) => void;
  canAsk: boolean;
  asking: boolean;
  onAsk: (question: string) => void;
}) {
  const { draft, onDraft, canAsk, asking, onAsk } = props;
  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (!canAsk || !draft.trim()) return;
    onAsk(draft);
    onDraft("");
  };
  return (
    <form className={cls("sp-ask", !canAsk && "waiting")} onSubmit={onSubmit}>
      <Sparkle />
      <input
        value={draft}
        onChange={(e) => onDraft(e.target.value)}
        placeholder={asking ? "Thinking…" : "Ask anything about this document"}
        maxLength={MAX_QUESTION_LENGTH}
        spellCheck={false}
      />
      <button type="submit" disabled={!canAsk || !draft.trim()} title="Ask (Return)">
        <SendIcon />
      </button>
    </form>
  );
}

export function SavedBar({ path }: { path: string }) {
  return (
    <div className="sp-saved">
      <CheckIcon />
      <span className="sp-saved-name">Saved {path.split(/[\\/]/).pop()}</span>
      <button onClick={() => revealItemInDir(path).catch(() => {})}>
        <FolderIcon /> Show
      </button>
    </div>
  );
}

const clock = (seconds: number) => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;

// What the footer says about the document: how much reading it saved, or how long it is taking
function footerInfo(doc: Doc, busy: boolean, now: number): ReactNode {
  if (doc.stage === "done" && doc.cached) {
    return (
      <>
        <b>Instant</b> · summarized earlier
      </>
    );
  }
  if (doc.stage === "done") {
    const minutes = doc.words ? readingMinutes(doc.words) - 1 : 0;
    return minutes >= 2 ? (
      <>
        Saves you <b>~{minutes} min</b> of reading
      </>
    ) : (
      <>Summarized in {Math.max(1, Math.round(doc.seconds ?? 0))} s</>
    );
  }
  if (busy && doc.stage !== "failed" && doc.stage !== "waiting") {
    return <span className="sp-clock">{clock(Math.max(0, (now - doc.startedAt) / 1000))}</span>;
  }
  return null;
}

export function PanelFooter(props: {
  doc: Doc;
  busy: boolean;
  now: number;
  /** Summarize again, in `language` or (null) the document's own */
  onRedo: (language: string | null) => void;
}) {
  const { doc, busy, now, onRedo } = props;
  const other = doc.stage === "done" && !busy ? otherLanguage(doc) : null;
  return (
    <footer className="sp-foot">
      <span className="sp-private" title="Read and summarized on this Mac. Nothing is uploaded.">
        <LockIcon /> On this Mac
      </span>
      <span className="sp-foot-info">{footerInfo(doc, busy, now)}</span>
      {doc.cached && !busy && (
        <button
          className="sp-chip icon"
          onClick={() => onRedo(null)}
          title="Summarize again from scratch"
          aria-label="Summarize again"
        >
          <RetryIcon />
        </button>
      )}
      {other && (
        <button className="sp-chip" onClick={() => onRedo(other)} title="Write this summary in another language">
          <TranslateIcon /> {languageName(other)}
        </button>
      )}
    </footer>
  );
}
