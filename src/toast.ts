// What a notification bubble says (shown by src/ToastBubble.tsx in its own window, placed by
// src-tauri/src/toast.rs), and how the circle builds one: failures, and what a
// compression gained and cost.

export type ToastLine = { label?: string; text: string };

export type ToastData = {
  id: number;
  tone: "error" | "info";
  title: string;
  /** "4.2 MB → 610 KB" */
  headline?: string;
  /** "−85%" */
  stat?: string;
  gains?: string[];
  risks?: string[];
  /** One line per failed file */
  lines?: ToastLine[];
  /** Failed files not listed */
  more?: number;
  footer?: string;
  /** "Show" reveals the first */
  outputs?: string[];
};

let nextId = 1;

// Decimal units, as the circle shows sizes. An upload limit counts as met only under the
// decimal value too, which is the stricter reading of "1 MB".
const KB = 1000;
const MB = 1000 * KB;

export const formatBytes = (n: number) =>
  n >= MB ? `${(n / MB).toFixed(1)} MB` : n >= KB ? `${Math.round(n / KB)} KB` : `${n} B`;

/** First letter up, except in names that start lowercase on purpose (macOS, iPhone) */
const capitalised = (s: string) => (/^[a-z]+[A-Z]/.test(s) ? s : s.charAt(0).toUpperCase() + s.slice(1));

/** Failed files, with the reason each failed */
export function failureToast(errors: string[], total: number, outputs: string[]): ToastData {
  const failed = errors.length;
  const title =
    failed >= total ? (total === 1 ? "Couldn't convert" : `${failed} files failed`) : `${failed} of ${total} failed`;
  const lines: ToastLine[] = errors.slice(0, 3).map((error) => {
    // "\"photo.png\": the reason" names its file
    const m = error.match(/^"([^"]+)"[:,]?\s*(.*)$/s);
    return m ? { label: m[1], text: capitalised(m[2] || "couldn't be converted") } : { text: capitalised(error) };
  });
  return {
    id: nextId++,
    tone: "error",
    title,
    lines,
    more: failed > 3 ? failed - 3 : undefined,
    footer: outputs.length > 0 ? `${outputs.length} converted fine` : undefined,
    outputs: outputs.length > 0 ? outputs : undefined,
  };
}

/** Upload and attachment limits a smaller file now fits under, smallest first */
const LIMITS: [number, string][] = [
  [100 * KB, "Fits 100 KB upload limits"],
  [200 * KB, "Fits 200 KB upload limits"],
  [500 * KB, "Fits 500 KB upload limits"],
  [MB, "Fits 1 MB upload limits"],
  [2 * MB, "Fits 2 MB upload limits"],
  [16 * MB, "Small enough to send on WhatsApp"],
  [25 * MB, "Fits an email attachment"],
];

/** Parts of a level's note that are good news rather than a cost */
const GOOD = /^(looks the same|sounds the same|hard to tell apart|text untouched|text stays sharp|fine on screens|fine for music|fine on phones)$/i;

/** Costs a compress tool other than the levels always has */
const TOOL_RISKS: Record<string, string[]> = {
  "cmp-auto": ["Quality lowered to 65%"],
  "cmp-1m": ["Quality lowered until it fits 1 MB"],
  "cmp-500k": ["Quality lowered until it fits 500 KB"],
  "cmp-200k": ["Quality lowered until it fits 200 KB"],
  "cmp-100k": ["Quality lowered, maybe resized, to fit 100 KB"],
  "vid-compress": ["Softer picture"],
};

/**
 * What a compression gained and cost. `note` is the backend's note for the first file:
 * a level's "PNG → JPG · Fine detail softens · max 1920 px", or "Already compact".
 */
export function compressToast(opts: {
  title: string;
  format: string;
  before: number;
  after: number;
  note: string | null;
  files: number;
  outputs: string[];
}): ToastData {
  const { title, format, before, after, note, files, outputs } = opts;
  const id = nextId++;
  if (after >= before || note === "Already compact") {
    return {
      id,
      tone: "info",
      title: "Already compact",
      headline: formatBytes(before),
      gains: [files > 1 ? "These files were already as small as they get" : "This file was already as small as it gets"],
      risks: ["Nothing lost: you got an untouched copy"],
      outputs,
    };
  }
  const gains = [`${formatBytes(before - after)} saved`];
  const fits = files === 1 ? LIMITS.find(([limit]) => after <= limit && before > limit) : undefined;
  if (fits) gains.push(fits[1]);
  const risks: string[] = [];
  // "PNG → JPG · Fine detail softens · max 1920 px"
  const parts = (note ?? "").split(" · ").filter(Boolean);
  for (const part of parts) {
    const change = part.match(/^([A-Z0-9]+) → ([A-Z0-9]+)$/);
    if (change) {
      risks.push(`Now a ${change[2]} (was ${change[1]})`);
      continue;
    }
    for (const piece of part.split(", ")) {
      (GOOD.test(piece) ? gains : risks).push(capitalised(piece));
    }
  }
  // The levels' notes say what they cost; the other compress tools always cost the same
  const isLevel = ["cmp-light", "cmp-balanced", "cmp-strong", "cmp-extreme"].includes(format);
  if (!isLevel) {
    for (const risk of TOOL_RISKS[format] ?? []) if (!risks.includes(risk)) risks.push(risk);
    if (format === "cmp-lossless") gains.push("Not a single pixel changed");
  }
  if (risks.length === 0) risks.push("Nothing you'd notice");
  return {
    id,
    tone: "info",
    title,
    headline: `${formatBytes(before)} → ${formatBytes(after)}`,
    stat: `−${Math.round((1 - after / before) * 100)}%`,
    gains,
    risks,
    footer: "Your original is untouched",
    outputs,
  };
}
