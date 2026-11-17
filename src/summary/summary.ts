// State of the summary panel (SummaryPanel.tsx), built from the events the backend
// streams while it reads a document and Apple Intelligence writes about it
// (src-tauri/src/summary/mod.rs).

export type Fact = { label?: string; value?: string };

// Every field may be missing or half-written while the model streams
export type Summary = {
  type?: string;
  title?: string;
  tldr?: string;
  points?: string[];
  facts?: Fact[];
  actions?: string[];
  keywords?: string[];
  // Questions the document answers, offered as one-click questions
  questions?: string[];
};

/** A question asked about a document, and its answer as it is written */
export type Answer = { question: string; text: string; done: boolean; error?: string };

export type SummaryEvent =
  | { type: "start"; files: string[] }
  | { type: "restart"; file: number; language: string | null }
  | { type: "reading"; file: number; page: number; pages: number; listening: boolean }
  | { type: "read"; file: number; kind: string; pages: number; words: number; ocr: boolean }
  | { type: "peek"; file: number; sentences: string[] }
  | { type: "plan"; file: number; parts: number; kept: number }
  | { type: "part"; file: number; part: number; parts: number }
  | { type: "notes"; file: number; part: number; text: string }
  | { type: "writing"; file: number }
  | { type: "summary"; file: number; data: Summary }
  | {
      type: "done";
      file: number;
      data: Summary;
      language: string;
      documentLanguage: string | null;
      seconds: number;
      cached: boolean;
    }
  | { type: "failed"; file: number; code: string; message: string }
  | { type: "answer"; file: number; text: string; done: boolean; error: string | null }
  | { type: "finished" };

export type Stage = "waiting" | "reading" | "thinking" | "writing" | "done" | "failed";

export type Doc = {
  name: string;
  stage: Stage;
  kind?: string;
  pages?: number;
  words?: number;
  ocr?: boolean;
  // Reading progress: pages for PDFs and scans, parts for long documents
  page?: number;
  readPages?: number;
  // A recording being written down first: `page` is then percent done
  listening?: boolean;
  part?: number;
  parts?: number;
  kept?: number;
  notes: string[];
  // A few sentences from the document itself, shown until the summary has its points
  peek?: string[];
  summary: Summary | null;
  answers: Answer[];
  language?: string;
  documentLanguage?: string | null;
  // Being translated into this language (another language was picked)
  translating?: string | null;
  seconds?: number;
  // Summarized earlier: the file has not changed since
  cached?: boolean;
  error?: { code: string; message: string };
  startedAt: number;
};

export type PanelState = {
  docs: Doc[];
  active: number;
  // The user picked a tab: stop following the file being worked on
  pinned: boolean;
  running: boolean;
};

export const EMPTY: PanelState = { docs: [], active: 0, pinned: false, running: false };

const newDoc = (name: string): Doc => ({ name, stage: "waiting", notes: [], summary: null, answers: [], startedAt: Date.now() });

export function reduce(state: PanelState, e: SummaryEvent): PanelState {
  if (e.type === "start") {
    return { docs: e.files.map(newDoc), active: 0, pinned: false, running: true };
  }
  if (e.type === "finished") return { ...state, running: false };

  const index = e.file;
  const doc = state.docs[index];
  if (!doc) return state;
  let next: Doc = doc;
  switch (e.type) {
    case "restart":
      // Same file, same text: only what the model wrote starts over
      next = {
        ...newDoc(doc.name),
        kind: doc.kind,
        pages: doc.pages,
        words: doc.words,
        ocr: doc.ocr,
        stage: "reading",
        translating: e.language,
        documentLanguage: doc.documentLanguage,
        answers: doc.answers,
      };
      break;
    case "reading":
      next = {
        ...doc,
        stage: "reading",
        page: e.page,
        readPages: e.pages,
        listening: e.listening,
        startedAt: doc.stage === "waiting" ? Date.now() : doc.startedAt,
      };
      break;
    case "read":
      next = { ...doc, kind: e.kind, pages: e.pages, words: e.words, ocr: e.ocr };
      break;
    case "peek":
      next = { ...doc, peek: e.sentences.length ? e.sentences : undefined };
      break;
    case "plan":
      next = { ...doc, stage: "thinking", parts: e.parts, kept: e.kept };
      break;
    case "part":
      next = { ...doc, stage: "thinking", part: e.part, parts: e.parts };
      break;
    case "notes": {
      const notes = [...doc.notes];
      notes[e.part - 1] = e.text;
      next = { ...doc, notes };
      break;
    }
    case "writing":
      next = { ...doc, stage: "writing" };
      break;
    case "summary":
      next = { ...doc, stage: "writing", summary: e.data };
      break;
    case "done":
      next = {
        ...doc,
        stage: "done",
        summary: e.data,
        language: e.language,
        documentLanguage: e.documentLanguage ?? doc.documentLanguage,
        translating: null,
        seconds: e.seconds || doc.seconds,
        cached: e.cached,
      };
      break;
    case "failed":
      next = { ...doc, stage: "failed", error: { code: e.code, message: e.message } };
      break;
    case "answer": {
      // Always for the latest question (one is asked at a time)
      const answers = [...doc.answers];
      const last = answers[answers.length - 1];
      if (last && !last.done) answers[answers.length - 1] = { ...last, text: e.text, done: e.done, error: e.error ?? undefined };
      next = { ...doc, answers };
      break;
    }
  }
  const docs = state.docs.map((d, i) => (i === index ? next : d));
  // Follow the file being worked on, unless the user chose one to look at
  const working = e.type !== "done" && e.type !== "failed" && e.type !== "answer";
  const active = e.type === "restart" ? index : !state.pinned && working ? index : state.active;
  return { ...state, docs, active, running: e.type === "restart" ? true : state.running };
}

// ─── Presentation helpers ───────────────────────────────────────────────

const EXTENSION_BADGES: [RegExp, string][] = [
  [/\.pdf$/i, "PDF"],
  [/\.(docx?|docm|dotx?|odt|rtf)$/i, "DOC"],
  [/\.(pptx?|ppsx?|pps|potx?|odp)$/i, "PPT"],
  [/\.(xlsx?|xlsm|xltx?|ods|csv|tsv)$/i, "XLS"],
  [/\.(md|markdown)$/i, "MD"],
  [/\.(txt|text|log|tex|typ)$/i, "TXT"],
  [/\.html?$/i, "WEB"],
  [/\.epub$/i, "BOOK"],
  [/\.(json|ya?ml|xml)$/i, "DATA"],
  // Spoken: transcribed first
  [/\.(mp3|wav|m4a|aac|flac|ogg|oga|opus|aiff?|caf|wma|amr|m4b|weba)$/i, "AUD"],
  [/\.(mp4|mov|m4v|mkv|webm|avi|flv|wmv|mpe?g|ts|3gp|ogv)$/i, "VID"],
];

/// A document worth waking the model up for when it is dragged in (photos rarely are
/// summarized, and loading the model costs memory)
export const isDocument = (path: string) => EXTENSION_BADGES.some(([re]) => re.test(path));

/// Short label and colour of the file's badge, from its name until the backend knows more
export function badgeOf(name: string): { label: string; tone: string } {
  const label = EXTENSION_BADGES.find(([re]) => re.test(name))?.[1] ?? "IMG";
  const tone: Record<string, string> = {
    PDF: "red",
    DOC: "blue",
    PPT: "orange",
    XLS: "green",
    MD: "gray",
    TXT: "gray",
    WEB: "teal",
    BOOK: "yellow",
    DATA: "teal",
    AUD: "pink",
    VID: "pink",
    IMG: "purple",
  };
  return { label, tone: tone[label] };
}

const plural = (n: number, word: string) => `${n.toLocaleString()} ${word}${n === 1 ? "" : "s"}`;

/// "PDF · 12 pages · 4,210 words"
export function metaOf(doc: Doc): string {
  const parts: string[] = [];
  if (doc.kind) parts.push(doc.ocr ? `${doc.kind} · scanned` : doc.kind);
  if (doc.pages && doc.pages > 1) parts.push(plural(doc.pages, "page"));
  if (doc.words) parts.push(plural(doc.words, "word"));
  return parts.join(" · ");
}

/// Minutes a person needs to read `words` (about 230 words a minute)
export const readingMinutes = (words: number) => Math.max(1, Math.round(words / 230));

export function languageName(code: string | null | undefined): string | null {
  if (!code) return null;
  try {
    return new Intl.DisplayNames(["en"], { type: "language" }).of(code) ?? code;
  } catch {
    return code;
  }
}

/// The language worth offering besides the summary's own: the system's when it differs,
/// else the document's (when the summary was translated earlier)
export function otherLanguage(doc: Doc): string | null {
  const system = navigator.language.split("-")[0].toLowerCase();
  const current = doc.language?.toLowerCase();
  if (!current) return null;
  if (system && system !== current) return system;
  const original = doc.documentLanguage?.toLowerCase();
  return original && original !== current ? original : null;
}

// Amounts, percentages and dates stand out in the text (Indonesian and English spellings)
const MONTHS =
  "Jan(?:uari|uary)?|Feb(?:ruari|ruary)?|Mar(?:et|ch)?|Apr(?:il)?|Mei|May|Jun[ei]?|Jul[iy]?|Agu(?:stus)?|Aug(?:ust)?|Sep(?:tember)?|Okt(?:ober)?|Oct(?:ober)?|Nov(?:ember)?|Des(?:ember)?|Dec(?:ember)?";
const FIGURE = new RegExp(
  [
    String.raw`(?:Rp|IDR|USD|US\$|\$|€|£|¥)\s?\d[\d.,]*(?:\s?(?:juta|miliar|triliun|ribu|million|billion|trillion|thousand|[mbk]n?)\b)?`,
    String.raw`\b\d[\d.,]*\s?(?:%|persen\b|percent\b)`,
    String.raw`\b\d[\d.,]*\s?(?:juta|miliar|triliun|million|billion|trillion)\b`,
    String.raw`\b\d{1,2}\s(?:${MONTHS})\s\d{4}\b`,
    String.raw`\b(?:${MONTHS})\s\d{1,2},?\s\d{4}\b`,
  ].join("|"),
  "gi"
);

export type Piece = { text: string; figure: boolean };

/// Splits text so the figures in it can be shown highlighted
export function figures(text: string): Piece[] {
  const out: Piece[] = [];
  let last = 0;
  for (const match of text.matchAll(FIGURE)) {
    const at = match.index ?? 0;
    if (at > last) out.push({ text: text.slice(last, at), figure: false });
    out.push({ text: match[0], figure: true });
    last = at + match[0].length;
  }
  if (last < text.length) out.push({ text: text.slice(last), figure: false });
  return out;
}

/// The last field the model has started on, which gets the typing caret
export function writingField(summary: Summary | null): keyof Summary | null {
  if (!summary) return null;
  const order: (keyof Summary)[] = ["type", "title", "tldr", "points", "facts", "actions", "keywords", "questions"];
  let last: keyof Summary | null = null;
  for (const key of order) if (summary[key] !== undefined) last = key;
  return last;
}

/// Friendly words for what went wrong, and what can be done about it
export function explain(code: string, message: string): { title: string; text: string; action?: "settings" | "retry" } {
  switch (code) {
    case "appleIntelligenceNotEnabled":
      return {
        title: "Apple Intelligence is off",
        text: "Turn it on in System Settings to get summaries. Everything still stays on this Mac.",
        action: "settings",
      };
    case "modelNotReady":
    case "assets":
      return {
        title: "Apple Intelligence is getting ready",
        text: "macOS is still downloading its model. Try again in a few minutes.",
        action: "retry",
      };
    case "deviceNotEligible":
    case "unsupported":
      return {
        title: "Not available on this Mac",
        text: "Summaries need Apple Intelligence: a Mac with Apple silicon and macOS 26 or later.",
      };
    case "unreadable":
      return { title: "Couldn't read this file", text: message.replace(/^"[^"]*":\s*/, "") };
    case "empty":
      return { title: "Nothing to read", text: "This file has no text to summarize." };
    case "guardrail":
    case "refusal":
      return {
        title: "Apple Intelligence declined",
        text: "Its safety rules don't allow a summary of this content.",
      };
    case "cancelled":
      return { title: "Stopped", text: "The panel was closed before the summary was finished.", action: "retry" };
    case "busy":
      return { title: "Apple Intelligence is busy", text: "Too many requests at once. Try again in a moment.", action: "retry" };
    case "language":
      return { title: "Language not supported", text: "Apple Intelligence can't read this document's language yet." };
    default:
      return { title: "Something went wrong", text: message || "The summary could not be finished.", action: "retry" };
  }
}
