import {
  useEffect,
  useReducer,
  useRef,
  useState,
  type CSSProperties,
  type FormEvent,
  type MouseEvent,
  type ReactNode,
} from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  EMPTY,
  badgeOf,
  explain,
  figures,
  languageName,
  metaOf,
  otherLanguage,
  readingMinutes,
  reduce,
  writingField,
  type Answer,
  type Doc,
  type PanelState,
  type Summary,
  type SummaryEvent,
} from "./summary";
import "./SummaryPanel.css";

// The panel's own window (src-tauri/src/summary/window.rs): a rounded glass card
const RADIUS = 22;

type Action =
  | SummaryEvent
  | { type: "pick"; file: number }
  | { type: "closed" }
  | { type: "ask"; file: number; question: string };

function reducer(state: PanelState, action: Action): PanelState {
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

const cls = (...names: (string | false | null | undefined)[]) => names.filter(Boolean).join(" ");

// ─── Icons ──────────────────────────────────────────────────────────────

const Icon = ({ children, className }: { children: ReactNode; className?: string }) => (
  <svg className={cls("sp-icon", className)} viewBox="0 0 24 24" aria-hidden="true">
    {children}
  </svg>
);

/** Sparkle: one large and one small four-pointed star, in pearl */
export const Sparkle = ({ className }: { className?: string }) => (
  <svg className={cls("sp-sparkle", className)} viewBox="0 0 24 24" aria-hidden="true">
    <path d="M10.5 3c.6 4.6 3.9 7.9 8.5 8.5-4.6.6-7.9 3.9-8.5 8.5-.6-4.6-3.9-7.9-8.5-8.5C6.6 10.9 9.9 7.6 10.5 3z" />
    <path d="M19 1.5c.25 1.7 1.3 2.75 3 3-1.7.25-2.75 1.3-3 3-.25-1.7-1.3-2.75-3-3 1.7-.25 2.75-1.3 3-3z" />
  </svg>
);

const CopyIcon = () => (
  <Icon>
    <rect x="9" y="9" width="11" height="11" rx="2.5" />
    <path d="M5 15V6.5A2.5 2.5 0 0 1 7.5 4H15" />
  </Icon>
);
const SaveIcon = () => (
  <Icon>
    <path d="M12 4v11M7.5 10.5 12 15l4.5-4.5M5 20h14" />
  </Icon>
);
const CloseIcon = () => (
  <Icon>
    <path d="M7 7l10 10M17 7 7 17" />
  </Icon>
);
const CheckIcon = () => (
  <Icon>
    <path d="m5.5 12.5 4 4L18.5 7.5" />
  </Icon>
);
const LockIcon = () => (
  <Icon>
    <rect x="5.5" y="11" width="13" height="9" rx="2.5" />
    <path d="M8.5 11V8.5a3.5 3.5 0 0 1 7 0V11" />
  </Icon>
);
const TranslateIcon = () => (
  <Icon>
    <path d="M4 6h9M8.5 4v2M11 6c-1 3.6-3.4 6.3-6.5 7.8M6.8 8.6c1 2.1 2.7 3.8 5 5" />
    <path d="m13 20 3.8-8.5L20.6 20M14.4 17.2h4.8" />
  </Icon>
);
const FolderIcon = () => (
  <Icon>
    <path d="M4 7.5A2.5 2.5 0 0 1 6.5 5H10l2 2.2h5.5A2.5 2.5 0 0 1 20 9.7v6.8a2.5 2.5 0 0 1-2.5 2.5h-11A2.5 2.5 0 0 1 4 16.5z" />
  </Icon>
);
const SendIcon = () => (
  <Icon>
    <path d="M12 19V5.5M6.5 11 12 5.5l5.5 5.5" />
  </Icon>
);
const QuoteIcon = () => (
  <Icon>
    <path d="M9.5 8.5c-2.4.8-4 2.9-4 5.5v2.5h4V12.5h-2M18.5 8.5c-2.4.8-4 2.9-4 5.5v2.5h4V12.5h-2" />
  </Icon>
);
const RetryIcon = () => (
  <Icon>
    <path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3" />
    <path d="M19.5 4.5v4.6h-4.6" />
  </Icon>
);

// ─── Pieces ─────────────────────────────────────────────────────────────

/** Text with its amounts, percentages and dates highlighted */
function Rich({ text }: { text: string }) {
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

const Caret = ({ on }: { on: boolean }) => (on ? <span className="sp-caret" aria-hidden="true" /> : null);

function Skeleton({ lines, className }: { lines: number; className?: string }) {
  return (
    <div className={cls("sp-skeleton", className)} aria-hidden="true">
      {Array.from({ length: lines }, (_, i) => (
        <span key={i} style={{ width: i === lines - 1 && lines > 1 ? "62%" : "100%" }} />
      ))}
    </div>
  );
}

function Section({ title, count, children }: { title: string; count?: number; children: ReactNode }) {
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

function FileCard({ doc }: { doc: Doc }) {
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

/** The document's own most telling sentences, there the moment it is read, while the
 *  model is still on its way to a summary */
function Peek({ sentences }: { sentences: string[] }) {
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

/** Before any of the summary exists: the model reading, with its notes as they come */
function Working({ doc }: { doc: Doc }) {
  let status = "Reading";
  let detail = "Apple Intelligence reads it right here on this Mac";
  let progress: number | undefined;
  if (doc.translating) {
    status = `Translating into ${languageName(doc.translating)}`;
    detail = "Names, numbers and dates stay exactly as they are";
  } else if (doc.stage === "writing") {
    status = "Writing the summary";
    detail = doc.parts ? `From notes on all ${doc.parts} parts` : "Picking out what matters";
  } else if (doc.stage === "thinking" && doc.parts) {
    status = `Taking notes · part ${doc.part ?? 1} of ${doc.parts}`;
    detail =
      doc.kept !== undefined && doc.kept < 0.98
        ? `A long one: reading its ${Math.max(1, Math.round(doc.kept * 100))}% most telling passages`
        : "A long one: reading it part by part";
    progress = ((doc.part ?? 1) - 1) / doc.parts;
  } else if (doc.listening) {
    status = doc.page ? `Listening · ${doc.page}%` : "Listening";
    detail = "Writing down what is said, right here on this Mac";
    progress = doc.page ? doc.page / 100 : undefined;
  } else if (doc.readPages && doc.readPages > 1) {
    status = `Reading page ${Math.min((doc.page ?? 0) + 1, doc.readPages)} of ${doc.readPages}`;
    detail = doc.ocr ? "Recognizing the scanned text" : "Getting the text out";
    progress = (doc.page ?? 0) / doc.readPages;
  }

  // The latest bullet points of the notes, newest last
  const lines = doc.notes
    .flatMap((note, part) =>
      note
        .split("\n")
        .map((line) => line.replace(/^\s*[-•*]\s*/, "").trim())
        .filter(Boolean)
        .map((text, i) => ({ key: `${part}-${i}`, text }))
    )
    .slice(doc.peek ? -2 : -4);

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

/** Questions asked about the document, each answer streaming in under its question */
function Answers({ answers }: { answers: Answer[] }) {
  return (
    <div className="sp-answers">
      {answers.map((answer, i) => (
        <div key={i} className="sp-qa">
          <div className="sp-question">{answer.question}</div>
          <div className={cls("sp-answer", answer.error && "error")}>
            <Sparkle />
            {answer.error ? (
              <span>{answer.error}</span>
            ) : answer.text ? (
              <span>
                <Rich text={answer.text} />
                <Caret on={!answer.done} />
              </span>
            ) : (
              <span className="sp-thinking">
                <i />
                <i />
                <i />
              </span>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}

function SummaryView({
  summary,
  writing,
  checked,
  onCheck,
  answers,
  onAsk,
  peek,
}: {
  summary: Summary;
  writing: boolean;
  peek?: string[];
  checked: Set<number>;
  onCheck: (index: number) => void;
  answers: Answer[];
  onAsk: ((question: string) => void) | null;
}) {
  const field = writing ? writingField(summary) : null;
  const points = (summary.points ?? []).filter((p) => p.trim());
  const facts = (summary.facts ?? []).filter((f) => f.value?.trim() || f.label?.trim());
  const actions = (summary.actions ?? []).filter((a) => a.trim());
  const keywords = (summary.keywords ?? []).filter((k) => k.trim());
  const started = (key: keyof Summary) => summary[key] !== undefined;

  return (
    <article className="sp-summary">
      {summary.type ? (
        <div className="sp-type">
          <Sparkle />
          {summary.type}
        </div>
      ) : (
        <Skeleton lines={1} className="eyebrow" />
      )}
      {summary.title ? (
        <h1 className="sp-title">
          {summary.title}
          <Caret on={field === "title"} />
        </h1>
      ) : (
        <Skeleton lines={1} className="title" />
      )}
      {summary.tldr ? (
        <p className="sp-tldr">
          <Rich text={summary.tldr} />
          <Caret on={field === "tldr"} />
        </p>
      ) : (
        writing && <Skeleton lines={2} className="card" />
      )}

      {points.length > 0 && (
        <Section title="Key points">
          <ol className="sp-points">
            {points.map((point, i) => (
              <li key={i}>
                <span className="sp-num">{i + 1}</span>
                <span>
                  <Rich text={point} />
                  <Caret on={field === "points" && i === points.length - 1} />
                </span>
              </li>
            ))}
          </ol>
        </Section>
      )}
      {writing && !started("points") && (peek ? <Peek sentences={peek} /> : started("tldr") && <Skeleton lines={3} />)}

      {facts.length > 0 && (
        <Section title="Key figures">
          <div className="sp-facts">
            {facts.map((fact, i) => (
              <div key={i} className={cls("sp-fact", (fact.value?.length ?? 0) > 24 && "wide")}>
                <div className="sp-fact-value">{fact.value || "…"}</div>
                <div className="sp-fact-label">{fact.label}</div>
              </div>
            ))}
          </div>
        </Section>
      )}

      {actions.length > 0 && (
        <Section title="To do" count={actions.length}>
          <ul className="sp-todo">
            {actions.map((action, i) => (
              <li key={i} className={checked.has(i) ? "done" : ""} onClick={() => onCheck(i)}>
                <span className="sp-check">{checked.has(i) && <CheckIcon />}</span>
                <span>
                  <Rich text={action} />
                  <Caret on={field === "actions" && i === actions.length - 1} />
                </span>
              </li>
            ))}
          </ul>
        </Section>
      )}

      {keywords.length > 0 && (
        <div className="sp-keywords">
          {keywords.map((keyword, i) => (
            <span key={i}>#{keyword.replace(/^#/, "")}</span>
          ))}
        </div>
      )}
      {writing && (
        <div className="sp-writing">
          <Sparkle className="live" />
          Writing…
        </div>
      )}

      {!writing && (answers.length > 0 || (summary.questions ?? []).some((q) => q.trim())) && (
        <Section title="Ask">
          {answers.length > 0 && <Answers answers={answers} />}
          {onAsk && (
            <div className="sp-suggestions">
              {(summary.questions ?? [])
                .filter((q) => q.trim() && !answers.some((a) => a.question === q))
                .map((question, i) => (
                  <button key={i} onClick={() => onAsk(question)}>
                    {question}
                  </button>
                ))}
            </div>
          )}
        </Section>
      )}
    </article>
  );
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

function Empty() {
  return (
    <div className="sp-empty">
      <div className="sp-orb calm" aria-hidden="true">
        <span />
      </div>
      <h2>Drop a document here</h2>
      <p>PDF, Word, PowerPoint, Excel, web pages, e-books, a photo of text, or a recording or video. Apple Intelligence explains it right here, on this Mac.</p>
    </div>
  );
}

const clock = (seconds: number) => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;

// ─── Panel ──────────────────────────────────────────────────────────────

export default function SummaryPanel() {
  const [state, dispatch] = useReducer(reducer, EMPTY);
  const [native, setNative] = useState(false);
  const [dropping, setDropping] = useState(false);
  const [flash, setFlash] = useState<"copied" | "saved" | null>(null);
  const [saved, setSaved] = useState<Record<number, string>>({});
  const [checked, setChecked] = useState<Record<number, Set<number>>>({});
  const [notice, setNotice] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());
  const runs = useRef(0);
  const owner = useRef(0);
  const flashTimer = useRef<number | undefined>(undefined);

  const doc: Doc | undefined = state.docs[state.active];
  const busy = state.running;
  const finished = doc?.stage === "done";

  // Each run streams on its own channel; only the latest run's events are shown
  // A question belongs to the panel as it is now: its channel takes over at once
  const follow = (start: (onEvent: Channel<SummaryEvent>) => Promise<unknown>, own = false) => {
    const id = ++runs.current;
    if (own) owner.current = id;
    const onEvent = new Channel<SummaryEvent>();
    onEvent.onmessage = (event) => {
      if (event.type === "start" || event.type === "restart") {
        owner.current = id;
        // What was saved or ticked belonged to the summary that is now being replaced
        const fresh = <T,>(all: Record<number, T>) =>
          event.type === "start" ? {} : Object.fromEntries(Object.entries(all).filter(([file]) => Number(file) !== event.file));
        setSaved(fresh);
        setChecked(fresh);
      }
      if (owner.current === id) dispatch(event);
    };
    start(onEvent).catch((e) => setNotice(String(e)));
  };
  const pickUp = () => follow((onEvent) => invoke("summary_run", { onEvent }));
  const redo = (file: number, language: string | null) =>
    follow((onEvent) => invoke("summary_redo", { file, language, onEvent }));

  // The job the circle handed over, now and whenever another one arrives
  useEffect(() => {
    pickUp();
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    listen("summary-job", pickUp).then((fn) => (cancelled ? fn() : (unlisten = fn)));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Native glass behind the whole card (macOS)
  useEffect(() => {
    const apply = () =>
      invoke<boolean>("set_glass", {
        cx: window.innerWidth / 2,
        cy: window.innerHeight / 2,
        diameter: 0,
        width: window.innerWidth,
        height: window.innerHeight,
        radius: RADIUS,
      })
        .then(setNative)
        .catch(() => setNative(false));
    apply();
    window.addEventListener("resize", apply);
    return () => window.removeEventListener("resize", apply);
  }, []);

  // Another document dropped on the panel itself is summarized right here
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    getCurrentWebview()
      .onDragDropEvent(({ payload }) => {
        if (payload.type === "enter" || payload.type === "over") setDropping(true);
        else if (payload.type === "leave") setDropping(false);
        else if (payload.type === "drop") {
          setDropping(false);
          invoke("summarize", { paths: payload.paths, toDownloads: false }).catch((e) => setNotice(String(e)));
        }
      })
      .then((fn) => (cancelled ? fn() : (unlisten = fn)));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // The clock in the footer ticks while the model works
  useEffect(() => {
    if (!busy) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [busy]);

  const showFlash = (kind: "copied" | "saved") => {
    setFlash(kind);
    window.clearTimeout(flashTimer.current);
    flashTimer.current = window.setTimeout(() => setFlash(null), 1600);
  };

  const asking = !!doc?.answers.some((a) => !a.done);
  const canAsk = finished && !busy && !asking;
  const ask = (question: string) => {
    const q = question.trim();
    if (!q || !canAsk) return;
    const file = state.active;
    dispatch({ type: "ask", file, question: q });
    follow((onEvent) => invoke("summary_ask", { file, question: q, onEvent }), true);
  };
  const [draft, setDraft] = useState("");
  const onAskSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (!canAsk || !draft.trim()) return;
    ask(draft);
    setDraft("");
  };

  // New questions and their answers scroll into view as they are written
  const bodyRef = useRef<HTMLDivElement>(null);
  const lastAnswer = doc?.answers[doc.answers.length - 1];
  useEffect(() => {
    const body = bodyRef.current;
    if (body && lastAnswer) body.scrollTo({ top: body.scrollHeight, behavior: "smooth" });
  }, [doc?.answers.length, lastAnswer?.text, lastAnswer?.done]);

  const close = () => {
    dispatch({ type: "closed" });
    invoke("summary_close").catch(() => {});
  };
  const copy = () => {
    if (!finished) return;
    invoke("summary_copy", { file: state.active })
      .then(() => showFlash("copied"))
      .catch((e) => setNotice(String(e)));
  };
  const save = () => {
    if (!finished) return;
    invoke<string>("summary_save", { file: state.active })
      .then((path) => {
        setSaved((s) => ({ ...s, [state.active]: path }));
        showFlash("saved");
      })
      .catch((e) => setNotice(String(e)));
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      const key = e.key.toLowerCase();
      const typing = (e.target as HTMLElement)?.tagName === "INPUT";
      if (typing && key === "escape" && draft) {
        // Esc in the question box clears it first, then closes the panel
        e.preventDefault();
        setDraft("");
      } else if (key === "escape" || (mod && key === "w")) {
        e.preventDefault();
        close();
      } else if (mod && key === "c" && !typing && !window.getSelection()?.toString()) {
        e.preventDefault();
        copy();
      } else if (mod && key === "s") {
        e.preventDefault();
        save();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 4000);
    return () => window.clearTimeout(timer);
  }, [notice]);

  // Drag the card by its header (buttons excepted)
  const onHeadDown = (e: MouseEvent) => {
    if (e.button === 0 && !(e.target as HTMLElement).closest("button")) getCurrentWindow().startDragging();
  };

  const toggleCheck = (index: number) =>
    setChecked((all) => {
      const set = new Set(all[state.active] ?? []);
      if (set.has(index)) set.delete(index);
      else set.add(index);
      return { ...all, [state.active]: set };
    });

  let body: ReactNode = null;
  if (doc) {
    const hasSummary = !!doc.summary && Object.keys(doc.summary).length > 0;
    if (doc.stage === "failed") body = <Failure doc={doc} onRetry={() => redo(state.active, null)} />;
    else if (hasSummary)
      body = (
        <SummaryView
          summary={doc.summary!}
          writing={doc.stage === "writing"}
          checked={checked[state.active] ?? new Set()}
          onCheck={toggleCheck}
          answers={doc.answers}
          onAsk={canAsk ? ask : null}
          peek={doc.translating ? undefined : doc.peek}
        />
      );
    else body = <Working doc={doc} />;
  }

  let footer: ReactNode = null;
  if (doc?.stage === "done" && doc.cached) {
    footer = (
      <>
        <b>Instant</b> · summarized earlier
      </>
    );
  } else if (doc?.stage === "done") {
    const minutes = doc.words ? readingMinutes(doc.words) - 1 : 0;
    footer =
      minutes >= 2 ? (
        <>
          Saves you <b>~{minutes} min</b> of reading
        </>
      ) : (
        <>Summarized in {Math.max(1, Math.round(doc.seconds ?? 0))} s</>
      );
  } else if (doc && busy && doc.stage !== "failed" && doc.stage !== "waiting") {
    footer = <span className="sp-clock">{clock(Math.max(0, (now - doc.startedAt) / 1000))}</span>;
  }
  const other = doc?.stage === "done" && !busy ? otherLanguage(doc) : null;
  const savedPath = saved[state.active];

  return (
    <div className={cls("sp", native && "native", busy && "busy", dropping && "dropping")}>
      <svg className="sp-defs" aria-hidden="true">
        <defs>
          <linearGradient id="sp-ai" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor="#ffffff" />
            <stop offset="0.55" stopColor="#d6dfeb" />
            <stop offset="1" stopColor="#ffffff" />
          </linearGradient>
        </defs>
      </svg>
      <div className="sp-glow edge" aria-hidden="true" />
      <div className="sp-glow haze" aria-hidden="true" />

      <header className="sp-head" onMouseDown={onHeadDown}>
        <div className="sp-brand">
          <Sparkle className={busy ? "live" : ""} />
          <div>
            <div className="sp-brand-name">Summary</div>
            <div className="sp-brand-sub">Apple Intelligence</div>
          </div>
        </div>
        <div className="sp-head-actions">
          <button className="sp-icon-button" title="Copy as text (⌘C)" disabled={!finished} onClick={copy}>
            {flash === "copied" ? <CheckIcon /> : <CopyIcon />}
          </button>
          <button className="sp-icon-button" title="Save next to the file (⌘S)" disabled={!finished} onClick={save}>
            {flash === "saved" ? <CheckIcon /> : <SaveIcon />}
          </button>
          <button className="sp-icon-button" title="Close (Esc)" onClick={close}>
            <CloseIcon />
          </button>
        </div>
      </header>

      {state.docs.length > 1 && (
        <nav className="sp-tabs">
          {state.docs.map((d, i) => (
            <button
              key={i}
              className={cls("sp-tab", i === state.active && "active", d.stage)}
              onClick={() => dispatch({ type: "pick", file: i })}
              title={d.name}
            >
              <span className="sp-tab-dot" />
              {d.name}
            </button>
          ))}
        </nav>
      )}

      {doc ? (
        <>
          <FileCard doc={doc} />
          <div className="sp-body" key={state.active} ref={bodyRef}>
            {body}
          </div>
          {finished && (
            <form className={cls("sp-ask", !canAsk && "waiting")} onSubmit={onAskSubmit}>
              <Sparkle />
              <input
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                placeholder={asking ? "Thinking…" : "Ask anything about this document"}
                maxLength={500}
                spellCheck={false}
              />
              <button type="submit" disabled={!canAsk || !draft.trim()} title="Ask (Return)">
                <SendIcon />
              </button>
            </form>
          )}
          {savedPath && (
            <div className="sp-saved">
              <CheckIcon />
              <span className="sp-saved-name">Saved {savedPath.split(/[\\/]/).pop()}</span>
              <button onClick={() => revealItemInDir(savedPath).catch(() => {})}>
                <FolderIcon /> Show
              </button>
            </div>
          )}
          <footer className="sp-foot">
            <span className="sp-private" title="Read and summarized on this Mac. Nothing is uploaded.">
              <LockIcon /> On this Mac
            </span>
            <span className="sp-foot-info">{footer}</span>
            {doc.cached && !busy && (
              <button
                className="sp-chip icon"
                onClick={() => redo(state.active, null)}
                title="Summarize again from scratch"
                aria-label="Summarize again"
              >
                <RetryIcon />
              </button>
            )}
            {other && (
              <button className="sp-chip" onClick={() => redo(state.active, other)} title="Write this summary in another language">
                <TranslateIcon /> {languageName(other)}
              </button>
            )}
          </footer>
        </>
      ) : (
        <Empty />
      )}

      {notice && <div className="sp-notice">{notice}</div>}
      {dropping && (
        <div className="sp-drop">
          <Sparkle className="live" />
          <span>Drop to summarize</span>
        </div>
      )}
    </div>
  );
}
