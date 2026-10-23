import { useEffect, useRef, useState, type CSSProperties, type MouseEvent } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { PhysicalPosition } from "@tauri-apps/api/dpi";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { DEBUG_DROP, DebugOverlay, type DropDebugInfo } from "./DebugOverlay";
import {
  CORE_OPEN_SCALE,
  CORE_SIZE,
  HOVER_POP,
  LABEL_RADIUS,
  RINGS,
  RING_OUTER,
  STAGE_SIZE,
  angleOf,
  labelOf,
  segmentPath,
  toCssPoint,
  zoneAt,
  type Point,
  type RingId,
} from "./dropTarget";
import "./App.css";

type Status = {
  title: string;
  hint: string;
  tone: "busy" | "ok" | "err";
  progress?: number; // 0..1 when known (several files, or the pages of one PDF); else a spinner
  outputs?: string[];
};

// Files or an image pasted with Cmd/Ctrl+V, waiting for the user to click a format.
type Picking = {
  paths: string[];
  image: boolean; // raw clipboard image (a temp file): the backend saves its result in Downloads
};

type Pasted = { kind: "files"; paths: string[] } | { kind: "image"; path: string } | { kind: "empty" };

type BatchResult = {
  outputs: string[];
  errors: string[];
  pages: number;
  bytes_before: number;
  bytes_after: number;
  note: string | null; // e.g. "BMP can't be compressed further"
};

const WORD_FILE = /\.(docx|docm|dotx|dotm|doc|dot)$/i;

// Dropping on a size preset keeps each file's own format (see SAME_FORMAT in convert.rs)
const SAME_FORMAT = "same";
type Progress = { done: number; total: number; unit: "file" | "page" };

// If the drop position itself resolves to nothing, a hover this recent still counts.
const HOVER_GRACE_MS = 150;
// Mouse travel (px) before a press on the core becomes a window drag instead of a click.
const DRAG_THRESHOLD = 3;

const RING_LIST = Object.values(RINGS);
const SEGMENT_PATHS = Object.fromEntries(
  RING_LIST.map((r) => [r.id, r.items.map((_, i) => segmentPath(r, i))])
) as Record<RingId, string[]>;

// Native glass disc (macOS): core-sized while idle, covers the whole ring while dragging.
const GLASS_IDLE = CORE_SIZE;
const GLASS_OPEN = (RING_OUTER + HOVER_POP) * 2;

// Progress ring drawn just outside the core
const PROGRESS_R = CORE_SIZE / 2 + 5;
const PROGRESS_LEN = 2 * Math.PI * PROGRESS_R;

// Approximates the CSS spring cubic-bezier(0.34, 1.56, 0.64, 1) used by the segments.
const easeOutBack = (t: number) => 1 + 2.2 * (t - 1) ** 3 + 1.2 * (t - 1) ** 2;
const easeInOut = (t: number) => (t < 0.5 ? 4 * t ** 3 : 1 - (-2 * t + 2) ** 3 / 2);

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

const formatBytes = (n: number) =>
  n >= 1e6 ? `${(n / 1e6).toFixed(1)} MB` : n >= 1e3 ? `${Math.round(n / 1e3)} KB` : `${n} B`;

// Option/Alt presets: macOS and Windows only (see src-tauri/src/keys.rs)
const IS_MAC = /Mac/i.test(navigator.userAgent);
const ALT_SUPPORTED = IS_MAC || /Windows/i.test(navigator.userAgent);
const ALT_KEY = IS_MAC ? "⌥" : "Alt";
const ALT_POLL_MS = 80;

// "Clipboard 2026-10-20 at 14.03.12", in local time (matches the macOS screenshot style)
const clipboardName = (d = new Date()) => {
  const p = (n: number) => String(n).padStart(2, "0");
  return `Clipboard ${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} at ${p(d.getHours())}.${p(d.getMinutes())}.${p(d.getSeconds())}`;
};

export default function App() {
  const [dragging, setDragging] = useState(false);
  const [picking, setPicking] = useState<Picking | null>(null);
  // Size/quality preset chosen with Option/Alt for the current drop, and the tray defaults
  const [preset, setPreset] = useState<string | null>(null);
  const presetRef = useRef<string | null>(null);
  const [summary, setSummary] = useState<string | null>(null);
  const altRef = useRef(false);
  const ringBeforeAlt = useRef<RingId>("main");
  const lastPoint = useRef<Point | null>(null);
  const [fileCount, setFileCount] = useState(0);
  // Several *images* dropped on PDF are merged into one; Word documents become one PDF each
  const [mergeable, setMergeable] = useState(false);
  const [hovered, setHovered] = useState<string | null>(null);
  const [ringId, setRingId] = useState<RingId>("main");
  const [status, setStatus] = useState<Status | null>(null);
  const resetTimer = useRef<number | undefined>(undefined);
  const [debugInfo, setDebugInfo] = useState<DropDebugInfo | null>(null);
  const coreRef = useRef<HTMLDivElement>(null);
  const scaleRef = useRef(window.devicePixelRatio || 1);
  const lastHover = useRef<{ id: string; at: number } | null>(null);
  // Drag handlers are synchronous, so the current ring and last segment live in refs too.
  const ringRef = useRef<RingId>("main");
  const lastIndex = useRef<number | null>(null);
  const press = useRef<{ x: number; y: number } | null>(null);
  const [nativeGlass, setNativeGlass] = useState(false);
  const glassSize = useRef(GLASS_IDLE);
  const draggingRef = useRef(false);
  draggingRef.current = dragging;
  const statusRef = useRef<Status | null>(null);
  statusRef.current = status;
  // The ring is open while dragging files in, or while choosing a format for a paste.
  const open = dragging || picking !== null;

  const coreCenter = () => {
    const rect = coreRef.current?.getBoundingClientRect();
    return rect
      ? { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 }
      : { x: window.innerWidth / 2, y: window.innerHeight / 2 };
  };

  const sendGlass = (diameter: number) => {
    const c = coreCenter();
    return invoke<boolean>("set_glass", { cx: c.x, cy: c.y, diameter });
  };

  const choosePreset = (id: string | null) => {
    presetRef.current = id;
    setPreset(id);
  };

  const switchRing = (id: RingId) => {
    ringRef.current = id;
    setRingId(id);
  };

  // Native glass is only available on macOS; elsewhere the command returns false
  useEffect(() => {
    sendGlass(GLASS_IDLE)
      .then(setNativeGlass)
      .catch(() => setNativeGlass(false));
  }, []);

  // Grow / shrink the glass disc in step with the ring animation
  useEffect(() => {
    if (!nativeGlass) return;
    const from = glassSize.current;
    const to = open ? GLASS_OPEN : GLASS_IDLE;
    if (from === to) return;
    const duration = open ? 420 : 260;
    const ease = open ? easeOutBack : easeInOut;
    const start = performance.now();
    let frame = 0;

    const step = (now: number) => {
      const t = Math.min(1, (now - start) / duration);
      glassSize.current = from + (to - from) * ease(t);
      sendGlass(glassSize.current);
      if (t < 1) frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
    return () => cancelAnimationFrame(frame);
  }, [open, nativeGlass]);

  // Maps a point (CSS px) to the selected format, for both drag & drop and paste-and-click.
  // "over"/"move" may swap rings; "drop" may fall back to a hover from the last moment.
  const resolve = (type: "enter" | "over" | "drop" | "move" | "click", point: Point, raw: Point = point) => {
    const center = coreCenter();
    let detail = zoneAt(point, center, RINGS[ringRef.current]);

    // Entering a "More"/"Back" segment swaps the ring. Only on entering, so the
    // aligned "Back" under the cursor right after a swap does not swap straight back.
    const moving = type === "over" || type === "move";
    if (moving && detail.zone.kind === "item" && detail.zone.index !== lastIndex.current) {
      const nav = detail.zone.item.nav;
      if (nav) {
        switchRing(nav);
        detail = zoneAt(point, center, RINGS[nav]);
      }
    }
    lastIndex.current = detail.zone.kind === "item" ? detail.zone.index : null;
    if (moving) lastPoint.current = point;

    const item = detail.zone.kind === "item" ? detail.zone.item : null;
    // On the Option/Alt ring, pointing at a preset picks it for this drop
    if (item?.preset && (moving || type === "drop" || type === "click")) choosePreset(item.id);
    // Dropping right on a preset converts in the file's own format with that preset
    const id = item?.preset ? SAME_FORMAT : item && !item.disabled && !item.nav ? item.id : null;

    let result = id;
    if (type === "drop" && !id && detail.zone.kind !== "core") {
      // Never fall back when the user deliberately released over the core.
      const last = lastHover.current;
      if (last && performance.now() - last.at <= HOVER_GRACE_MS) result = last.id;
    }

    if (DEBUG_DROP) {
      const info: DropDebugInfo = {
        type,
        raw,
        dpr: window.devicePixelRatio,
        scaleFactor: scaleRef.current,
        point,
        center,
        ring: ringRef.current,
        detail,
        result: result ?? "—",
      };
      console.log("[drop-debug]", info);
      setDebugInfo(info);
    }
    return result;
  };

  const setHover = (id: string | null) => {
    if (id) lastHover.current = { id, at: performance.now() };
    setHovered(id);
  };

  const closeRing = () => {
    lastHover.current = null;
    lastIndex.current = null;
    lastPoint.current = null;
    altRef.current = false;
    choosePreset(null);
    switchRing("main");
    setDragging(false);
    setPicking(null);
    setHovered(null);
  };

  const runBatch = async (paths: string[], format: string, toDownloads = false, preset?: string | null) => {
    window.clearTimeout(resetTimer.current);
    const same = format === SAME_FORMAT;
    const presetLabel = preset && !same ? ` · ${labelOf(preset)}` : "";
    const label = same ? (labelOf(preset ?? "") ?? "Done") : (labelOf(format) ?? format.toUpperCase()) + presetLabel;
    const many = paths.length > 1;
    setStatus({
      title: many ? `0/${paths.length}` : "Converting",
      hint: `→ ${label}`,
      tone: "busy",
      progress: many ? 0 : undefined,
    });

    const onProgress = new Channel<Progress>();
    onProgress.onmessage = ({ done, total, unit }) =>
      setStatus((s) => {
        if (s?.tone !== "busy") return s;
        // A single PDF reports its pages; several files report files
        if (unit === "page" && total > 1) return { ...s, title: `Page ${done}/${total}`, progress: done / total };
        if (unit === "file" && many) return { ...s, title: `${done}/${total}`, progress: done / total };
        return s;
      });

    let result: BatchResult;
    try {
      result = await invoke<BatchResult>("convert_batch", {
        paths,
        format,
        onProgress,
        toDownloads,
        preset: preset ?? null,
      });
    } catch (e) {
      result = { outputs: [], errors: [String(e)], pages: 0, bytes_before: 0, bytes_after: 0, note: null };
    }

    const { outputs, errors, pages, bytes_before, bytes_after, note } = result;
    const merged = format === "pdf" && many && outputs.length === 1;
    // Resizing/compressing in place: the size change is what the user wants to see
    const sizes = (same || preset === "compress") && bytes_before > 0;
    const done = sizes
      ? `${note ? `${note} · ` : bytes_after >= bytes_before ? "Already compact · " : ""}${formatBytes(bytes_before)} → ${formatBytes(bytes_after)}`
      : merged
      ? `${plural(paths.length - errors.length, "image")} → 1 PDF${presetLabel}`
      : pages > 0 // PDF pages rendered to images (multi-page PDFs give a folder each)
        ? `${plural(pages, "page")} → ${label}`
        : `${plural(outputs.length, "file")} → ${label}`;

    if (errors.length === 0) {
      // A note (e.g. which Word engine was used) is shown unless it is already part of `done`
      const extra = note && !sizes ? ` · ${note}` : "";
      setStatus({ title: "✓ Done", hint: `${done}${extra} · click to show`, tone: "ok", outputs });
    } else {
      setStatus({
        title: outputs.length > 0 ? `${paths.length - errors.length} done, ${errors.length} failed` : "Failed",
        hint: errors[0],
        tone: "err",
        outputs,
      });
    }

    resetTimer.current = window.setTimeout(() => setStatus(null), outputs.length > 0 ? 6000 : 3500);
  };

  // Drag & drop file
  useEffect(() => {
    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];
    const keep = (fn: UnlistenFn) => (cancelled ? fn() : unlisteners.push(fn));

    // Drag events are synchronous, so keep the current scale factor at hand.
    // It changes when the window moves to a monitor with different scaling.
    const win = getCurrentWindow();
    const refreshScale = () =>
      win.scaleFactor().then((f) => (scaleRef.current = f)).catch(() => {});
    refreshScale();
    win.onScaleChanged(({ payload }) => (scaleRef.current = payload.scaleFactor)).then(keep);

    const at = (position: PhysicalPosition) => ({
      point: toCssPoint(position, scaleRef.current),
      raw: { x: position.x, y: position.y },
    });

    getCurrentWebview()
      .onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === "enter") {
          refreshScale();
          closeRing(); // a drag replaces any pending paste
          setDragging(true);
          setFileCount(p.paths.length);
          setMergeable(p.paths.length > 1 && p.paths.every((path) => !WORD_FILE.test(path)));
          const { point, raw } = at(p.position);
          setHover(resolve(p.type, point, raw));
        } else if (p.type === "over") {
          const { point, raw } = at(p.position);
          setHover(resolve(p.type, point, raw));
        } else if (p.type === "leave") {
          if (DEBUG_DROP) console.log("[drop-debug] leave");
          closeRing();
        } else if (p.type === "drop") {
          const { point, raw } = at(p.position);
          const format = resolve(p.type, point, raw);
          const chosen = presetRef.current;
          closeRing();
          if (format) runBatch(p.paths, format, false, chosen);
        }
      })
      .then(keep);

    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
    };
  }, []);

  // Holding Option/Alt while the ring is open swaps in the size/quality presets.
  // Polled, because OS drag events carry no modifier keys (see keys.rs).
  useEffect(() => {
    if (!open || !ALT_SUPPORTED) return;
    const timer = window.setInterval(async () => {
      const down = await invoke<boolean>("option_key_down").catch(() => false);
      if (down === altRef.current) return;
      altRef.current = down;
      if (down) ringBeforeAlt.current = ringRef.current === "size" ? "main" : ringRef.current;
      switchRing(down ? "size" : ringBeforeAlt.current);
      lastIndex.current = null;
      // Re-evaluate where the cursor already is, so nothing waits for the next mouse move
      setHover(lastPoint.current ? resolve("over", lastPoint.current) : null);
    }, ALT_POLL_MS);
    return () => window.clearInterval(timer);
  }, [open]);

  // Tray resize/quality defaults, shown under the core when not the defaults
  useEffect(() => {
    invoke<string | null>("settings_summary").then(setSummary).catch(() => {});
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    listen<string | null>("settings-changed", ({ payload }) => setSummary(payload)).then((fn) =>
      cancelled ? fn() : (unlisten = fn)
    );
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // "Undo Last Conversion" from the tray menu
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    listen<{ count: number; error: string | null }>("history-undone", ({ payload }) => {
      window.clearTimeout(resetTimer.current);
      setStatus(
        payload.error
          ? { title: "Undo failed", hint: payload.error, tone: "err" }
          : { title: "↩ Undone", hint: `${plural(payload.count, "file")} moved to Trash`, tone: "ok" }
      );
      resetTimer.current = window.setTimeout(() => setStatus(null), 3500);
    }).then((fn) => (cancelled ? fn() : (unlisten = fn)));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const paste = async () => {
    let pasted: Pasted;
    try {
      pasted = await invoke<Pasted>("read_clipboard", { name: clipboardName() });
    } catch (e) {
      pasted = { kind: "empty" };
      console.error("read_clipboard", e);
    }

    if (pasted.kind === "empty") {
      window.clearTimeout(resetTimer.current);
      setStatus({ title: "Nothing to paste", hint: "Copy an image or files first", tone: "err" });
      resetTimer.current = window.setTimeout(() => setStatus(null), 3000);
      return;
    }

    window.clearTimeout(resetTimer.current);
    setStatus(null);
    switchRing("main");
    lastIndex.current = null;
    if (pasted.kind === "files") {
      setFileCount(pasted.paths.length);
      setMergeable(pasted.paths.length > 1 && pasted.paths.every((path) => !WORD_FILE.test(path)));
      setPicking({ paths: pasted.paths, image: false });
    } else {
      setFileCount(1);
      setPicking({ paths: [pasted.path], image: true });
    }
  };

  // Cmd+Q quits, Cmd/Ctrl+V pastes, Esc cancels a paste
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      const key = e.key.toLowerCase();
      if (mod && key === "q") {
        e.preventDefault();
        invoke("quit_app");
      } else if (mod && key === "v") {
        e.preventDefault();
        if (!draggingRef.current && statusRef.current?.tone !== "busy") paste();
      } else if (key === "escape") {
        closeRing();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // The core both moves the window (press + drag) and opens the result (click).
  // data-tauri-drag-region would start a window drag on every press and swallow clicks.
  const onCoreDown = (e: MouseEvent) => {
    if (e.button === 0) press.current = { x: e.clientX, y: e.clientY };
  };
  const onCoreMove = (e: MouseEvent) => {
    const p = press.current;
    if (p && Math.hypot(e.clientX - p.x, e.clientY - p.y) > DRAG_THRESHOLD) {
      press.current = null;
      getCurrentWindow().startDragging();
    }
  };
  const onCoreUp = () => {
    if (!press.current) return;
    press.current = null;
    if (picking) {
      closeRing(); // clicking the center cancels a paste, like dropping on it
      return;
    }
    const first = status?.outputs?.[0];
    if (first) revealItemInDir(first).catch(() => {});
  };

  // While choosing a format for a paste, the mouse plays the role of the dragged file.
  const onPickMove = (e: MouseEvent) => {
    if (picking) setHover(resolve("move", { x: e.clientX, y: e.clientY }));
  };
  const onPickClick = (e: MouseEvent) => {
    if (!picking || coreRef.current?.contains(e.target as Node)) return;
    const format = resolve("click", { x: e.clientX, y: e.clientY });
    if (!format) return;
    const { paths, image } = picking;
    const chosen = presetRef.current;
    closeRing();
    runBatch(paths, format, image, chosen);
  };

  const hoveredLabel = hovered ? labelOf(hovered) : undefined;

  let title = "Convert";
  let hint = "Drop files here";
  const presetLabel = preset ? labelOf(preset) : undefined;
  let badge: string | null = summary;
  if (open && ringId === "size") {
    title = presetLabel ?? "Size";
    hint = presetLabel
      ? `${dragging ? "Drop" : "Click"}: same format, or release ${ALT_KEY}`
      : "Point to a size";
    badge = null;
  } else if (open) {
    const verb = dragging ? "Release" : "Click";
    badge = presetLabel ? `Size: ${presetLabel}` : ALT_SUPPORTED ? `Hold ${ALT_KEY} to resize` : summary;
    if (hoveredLabel) {
      title = hoveredLabel;
      hint =
        hovered === "clean"
          ? `${verb} to strip metadata`
          : hovered === "docx-ocr"
            ? `${verb} to read scanned text into Word`
          : hovered === "pdf" && fileCount > 1 && mergeable
            ? `${verb} to merge into 1 PDF`
            : `${verb} to convert`;
    } else {
      title = ringId === "more" ? "More" : picking?.image ? "Image" : plural(fileCount, "file");
      hint = dragging ? "Point to a format" : "Pick a format · Esc to cancel";
    }
  } else if (status) {
    title = status.title;
    hint = status.hint;
  }

  const tone = !open && status ? status.tone : "";
  const busy = !open && status?.tone === "busy";
  const progress = busy ? status?.progress : undefined;
  const clickable = !open && !!status?.outputs?.length;

  return (
    <main
      className={picking && hovered ? "pick-ready" : undefined}
      onMouseMove={onPickMove}
      onClick={onPickClick}
    >
      <div
        className={`stage ${open ? "open" : ""} ${nativeGlass ? "native" : ""}`}
        style={
          {
            "--stage": `${STAGE_SIZE}px`,
            "--core": `${CORE_SIZE}px`,
            "--core-open-scale": CORE_OPEN_SCALE,
          } as CSSProperties
        }
      >
        {/* Slice-shaped format segments. Selection is by direction (see zoneAt) */}
        <svg
          className="ring"
          viewBox={`${-STAGE_SIZE / 2} ${-STAGE_SIZE / 2} ${STAGE_SIZE} ${STAGE_SIZE}`}
          aria-hidden="true"
        >
          <defs>
            <linearGradient
              id="seg-rim"
              gradientUnits="userSpaceOnUse"
              x1={-RING_OUTER}
              y1={-RING_OUTER}
              x2={RING_OUTER}
              y2={RING_OUTER}
            >
              <stop offset="0" stopColor="#fff" stopOpacity="0.7" />
              <stop offset="0.4" stopColor="#fff" stopOpacity="0.12" />
              <stop offset="0.65" stopColor="#fff" stopOpacity="0.05" />
              <stop offset="1" stopColor="#fff" stopOpacity="0.35" />
            </linearGradient>
            <linearGradient id="seg-active" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0" stopColor="var(--accent)" stopOpacity="0.95" />
              <stop offset="1" stopColor="var(--accent-2)" stopOpacity="0.85" />
            </linearGradient>
          </defs>

          {RING_LIST.map((r) => (
            <g key={r.id} className={`ring-layer ${r.id === ringId ? "current" : ""}`}>
              {r.items.map((item, i) => {
                const angle = angleOf(r, i);
                const lx = Math.cos(angle) * LABEL_RADIUS;
                const ly = Math.sin(angle) * LABEL_RADIUS;
                const style = {
                  "--i": i,
                  "--dx": `${Math.cos(angle) * HOVER_POP}px`,
                  "--dy": `${Math.sin(angle) * HOVER_POP}px`,
                } as CSSProperties;
                const cls = [
                  "segment",
                  (hovered === item.id || (item.preset && preset === item.id)) && "active",
                  item.disabled && "disabled",
                  item.nav && "nav",
                ]
                  .filter(Boolean)
                  .join(" ");

                return (
                  <g key={item.id} className={cls} style={style}>
                    <g className="segment-inner">
                      <path className="segment-body" d={SEGMENT_PATHS[r.id][i]} />
                      <path className="segment-rim" d={SEGMENT_PATHS[r.id][i]} />
                      <text className="segment-label" x={lx} y={item.sub ? ly - 4 : ly}>
                        {item.label}
                      </text>
                      {item.sub && (
                        <text className="segment-sub" x={lx} y={ly + 8}>
                          {item.sub}
                        </text>
                      )}
                    </g>
                  </g>
                );
              })}
            </g>
          ))}
        </svg>

        {/* Drag the core to move the window, or click it to reveal the result */}
        <div
          ref={coreRef}
          className={`core glass ${tone} ${clickable ? "clickable" : ""}`}
          onMouseDown={onCoreDown}
          onMouseMove={onCoreMove}
          onMouseUp={onCoreUp}
          onMouseLeave={() => (press.current = null)}
        >
          {/* Determinate ring when progress is known, otherwise a spinning arc */}
          {busy && (
            <svg
              className={`progress ${progress === undefined ? "indeterminate" : ""}`}
              viewBox={`${-PROGRESS_R - 2} ${-PROGRESS_R - 2} ${PROGRESS_R * 2 + 4} ${PROGRESS_R * 2 + 4}`}
            >
              <circle className="progress-track" r={PROGRESS_R} />
              <circle
                className="progress-bar"
                r={PROGRESS_R}
                strokeDasharray={progress === undefined ? `${PROGRESS_LEN * 0.22} ${PROGRESS_LEN}` : PROGRESS_LEN}
                strokeDashoffset={progress === undefined ? 0 : PROGRESS_LEN * (1 - progress)}
              />
            </svg>
          )}
          <span className="core-title">{title}</span>
          <span className="core-hint">{hint}</span>
          {badge && <span className="core-badge">{badge}</span>}
        </div>
      </div>
      {DEBUG_DROP && <DebugOverlay info={debugInfo} />}
    </main>
  );
}
