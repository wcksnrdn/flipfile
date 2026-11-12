import { useEffect, useMemo, useRef, useState, type CSSProperties, type MouseEvent } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import type { PhysicalPosition } from "@tauri-apps/api/dpi";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import type { Update } from "@tauri-apps/plugin-updater";
import { checkForUpdate, type UpdateState } from "./updater";
import UpdateDialog from "./UpdateDialog";
import { DEBUG_DROP, DebugOverlay, type DropDebugInfo } from "./DebugOverlay";
import WorkshopPanel, { MAX_RING_SLOTS } from "./WorkshopPanel";
import {
  CORE_OPEN_SCALE,
  CORE_SIZE,
  HOVER_POP,
  LABEL_RADIUS,
  RING_OUTER,
  STAGE_SIZE,
  angleOf,
  isCategoryRing,
  labelOf,
  toCssPoint,
  zoneAt,
  type Point,
  type Ring,
  type RingId,
} from "./dropTarget";
import {
  AI_SUMMARY,
  buildWorkspaceRings,
  defaultSlots,
  getSlotsForRing,
  hasAiSupport,
  loadWorkspacesState,
  setAiSupport,
  saveWorkspacesState,
  type CatalogEntry,
  type WorkspacesState,
} from "./workshop";
import { isDocument } from "./summary";
import "./App.css";

type Status = {
  title: string;
  hint: string;
  tone: "busy" | "ok" | "err";
  progress?: number; // 0..1 when known (several files, or the pages of one PDF); else a spinner
  outputs?: string[];
  ai?: boolean; // an AI summary: the panel shows the result, clicking the circle brings it back
};

// What the summary panel reports to the circle (src-tauri/src/summary/mod.rs)
type SummaryProgress = {
  title: string;
  hint: string;
  tone: "busy" | "ok" | "err" | "idle";
  progress: number | null;
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

// Office documents, data tables, and audio/video files convert one by one; only images merge into one PDF
const OFFICE_FILE =
  /\.(docx|docm|dotx|dotm|doc|dot|xlsx|xlsm|xltx|xltm|xls|xlt|ods|csv|tsv|tab|json|yaml|yml|xml|odt|rtf|md|markdown|txt|text|log|html|htm|epub|pptx|pptm|ppsx|ppsm|potx|potm|ppt|pps|pot|odp|mp3|wav|wave|m4a|flac|ogg|oga|aif|aiff|opus|aac|caf|wma|mp4|m4v|mov|webm|mkv|avi|flv|wmv|mpg|mpeg|ts|mts|3gp)$/i;

// Dropping on a size preset keeps each file's own format (see SAME_FORMAT in convert.rs)
const SAME_FORMAT = "same";
type Progress = { done: number; total: number; unit: "file" | "page" | "step" };

// If the drop position itself resolves to nothing, a hover this recent still counts.
const HOVER_GRACE_MS = 150;
// Mouse travel (px) before a press on the core becomes a window drag instead of a click.
const DRAG_THRESHOLD = 3;

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
// Where the AI tools run: this computer, never a server
const ON_DEVICE = IS_MAC ? "On this Mac" : /Windows/i.test(navigator.userAgent) ? "On this PC" : "On this computer";
const ALT_POLL_MS = 80;

// "Clipboard 2026-10-20 at 14.03.12", in local time (matches the macOS screenshot style)
const clipboardName = (d = new Date()) => {
  const p = (n: number) => String(n).padStart(2, "0");
  return `Clipboard ${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} at ${p(d.getHours())}.${p(d.getMinutes())}.${p(d.getSeconds())}`;
};

// Category ring labels for the core center text
const CATEGORY_LABELS: Partial<Record<RingId, string>> = {
  category: "Convert",
  custom: "Custom",
  photo: "Photo",
  "photo-formats": "Formats",
  "photo-pro": "Pro / 3D",
  "photo-tools": "Tools",
  "photo-compress": "Compress",
  "photo-transform": "Transform",
  office: "Office",
  "office-docs": "Docs",
  "office-data": "Data",
  video: "Video",
  "video-tools": "Video Tools",
  music: "Music",
  "music-more": "More Audio",
  "music-transcribe": "Transcribe",
  "video-transcribe": "Subtitles",
};

type ChipDrag = {
  item: CatalogEntry;
  fromSlotIndex: number | null;
  x: number;
  y: number;
  targetSlice: number | "core" | null;
};

export default function App() {
  const [wsState, setWsState] = useState<WorkspacesState>(() => loadWorkspacesState());
  const [workshopOpen, setWorkshopOpen] = useState(false);
  const [editingRing, setEditingRing] = useState<RingId>("photo");
  const [chipDrag, setChipDrag] = useState<ChipDrag | null>(null);

  // In-app updater state
  const [updateInfo, setUpdateInfo] = useState<Update | null>(null);
  const [updateState, setUpdateState] = useState<UpdateState>({ status: "idle" });
  const [toastMessage, setToastMessage] = useState<string | null>(null);

  const showToast = (msg: string) => {
    setToastMessage(msg);
    window.setTimeout(() => setToastMessage(null), 3200);
  };

  const triggerUpdateCheck = async () => {
    setUpdateState({ status: "checking" });
    try {
      const up = await checkForUpdate();
      if (up) {
        setUpdateInfo(up);
        setUpdateState({ status: "available", update: up, version: up.version, body: up.body });
      } else {
        setUpdateState({ status: "up-to-date" });
        showToast("You're up to date · v0.8.0");
      }
    } catch (err) {
      setUpdateState({ status: "error", message: String(err) });
      showToast("Could not check for updates");
    }
  };

  const activeWorkspace = useMemo(
    () => wsState.workspaces.find((w) => w.id === wsState.activeId) ?? wsState.workspaces[0],
    [wsState]
  );
  // Rebuilt once the backend says whether AI summaries can run here (workshop.ts)
  const [aiSupported, setAiSupported] = useState(IS_MAC);
  const built = useMemo(() => buildWorkspaceRings(activeWorkspace), [activeWorkspace, aiSupported]);
  const ringsRef = useRef<Record<RingId, Ring>>(built.rings);
  ringsRef.current = built.rings;
  const entryRingRef = useRef<RingId>(built.entryRing);
  entryRingRef.current = built.entryRing;
  const ringList = useMemo(() => Object.values(built.rings), [built.rings]);

  const [dragging, setDragging] = useState(false);
  const [picking, setPicking] = useState<Picking | null>(null);
  // Size/quality preset chosen with Option/Alt for the current drop, and the tray defaults
  const [preset, setPreset] = useState<string | null>(null);
  const presetRef = useRef<string | null>(null);
  const [summary, setSummary] = useState<string | null>(null);
  const altRef = useRef(false);
  const ringBeforeAlt = useRef<RingId>(built.entryRing);
  const lastPoint = useRef<Point | null>(null);
  const [fileCount, setFileCount] = useState(0);
  // Several *images* dropped on PDF are merged into one; Word documents become one PDF each
  const [mergeable, setMergeable] = useState(false);
  const [hovered, setHovered] = useState<string | null>(null);
  const [ringId, setRingId] = useState<RingId>(built.entryRing);
  const [status, setStatus] = useState<Status | null>(null);
  const resetTimer = useRef<number | undefined>(undefined);
  const [debugInfo, setDebugInfo] = useState<DropDebugInfo | null>(null);
  const coreRef = useRef<HTMLDivElement>(null);
  const scaleRef = useRef(window.devicePixelRatio || 1);
  const lastHover = useRef<{ id: string; at: number } | null>(null);
  // Drag handlers are synchronous, so the current ring and last segment live in refs too.
  const ringRef = useRef<RingId>(built.entryRing);
  const lastIndex = useRef<number | null>(null);
  const press = useRef<{ x: number; y: number } | null>(null);
  const [nativeGlass, setNativeGlass] = useState(false);
  const glassSize = useRef(GLASS_IDLE);
  const draggingRef = useRef(false);
  draggingRef.current = dragging;
  const statusRef = useRef<Status | null>(null);
  statusRef.current = status;
  // The ring is open while dragging files in, choosing a format for a paste, or editing in the Workshop.
  const open = dragging || picking !== null || workshopOpen;

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

  const updateWorkspaces = (updater: (prev: WorkspacesState) => WorkspacesState) => {
    setWsState((prev) => {
      const next = updater(prev);
      saveWorkspacesState(next);
      return next;
    });
  };

  const toggleWorkshop = (nextOpen?: boolean) => {
    setWorkshopOpen((prev) => {
      const target = nextOpen ?? !prev;
      const win = getCurrentWindow();
      const w = target ? 940 : 320;
      const h = target ? 620 : 320;
      invoke("set_workshop_window", { open: target, width: w, height: h }).catch(() => {});
      if (target) {
        win
          .setResizable(true)
          .then(() => win.setMinSize(new LogicalSize(720, 480)))
          .then(() => win.setMaxSize(null))
          .then(() => win.setSize(new LogicalSize(w, h)))
          .catch(() => {});
      } else {
        win
          .setResizable(true)
          .then(() => win.setMinSize(new LogicalSize(320, 320)))
          .then(() => win.setMaxSize(new LogicalSize(320, 320)))
          .then(() => win.setSize(new LogicalSize(320, 320)))
          .then(() => win.setResizable(false))
          .catch(() => {});
      }

      if (target) {
        const ringToEdit: RingId = activeWorkspace.mode === "direct" ? "custom" : editingRing;
        switchRing(ringToEdit);
      } else {
        switchRing(entryRingRef.current);
        setHovered(null);
      }
      // Recenter native glass after window resize
      window.setTimeout(() => sendGlass(target ? GLASS_OPEN : GLASS_IDLE).catch(() => {}), 80);
      return target;
    });
  };

  // Keep native glass disc aligned with the ring center whenever the Workshop window is resized
  useEffect(() => {
    if (!workshopOpen) return;
    const onResize = () => {
      sendGlass(GLASS_OPEN).catch(() => {});
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [workshopOpen]);

  const updateSlotsForRing = (targetRing: RingId, nextSlots: string[]) => {
    updateWorkspaces((prev) => ({
      ...prev,
      workspaces: prev.workspaces.map((ws) => {
        if (ws.id !== prev.activeId) return ws;
        if (targetRing === "custom") {
          return { ...ws, customSlots: nextSlots };
        }
        return {
          ...ws,
          roomSlots: { ...ws.roomSlots, [targetRing]: nextSlots },
        };
      }),
    }));
  };

  const handleStartChipDrag = (
    item: CatalogEntry,
    fromSlotIndex: number | null,
    clientX: number,
    clientY: number
  ) => {
    const activeEditRing: RingId = activeWorkspace.mode === "direct" ? "custom" : editingRing;
    switchRing(activeEditRing);
    setChipDrag({ item, fromSlotIndex, x: clientX, y: clientY, targetSlice: null });

    const onMove = (ev: PointerEvent) => {
      const pt = { x: ev.clientX, y: ev.clientY };
      const center = coreCenter();
      const ringObj = ringsRef.current[activeEditRing];
      const slots = getSlotsForRing(activeWorkspace, activeEditRing);
      const editableCount = slots.length;
      const dist = Math.hypot(pt.x - center.x, pt.y - center.y);

      let targetSlice: number | "core" | null = null;
      if (dist <= RING_OUTER + 32) {
        const detail = zoneAt(pt, center, ringObj);
        if (detail.zone.kind === "core") {
          targetSlice = "core";
          setHovered(null);
        } else if (detail.zone.index < editableCount) {
          targetSlice = detail.zone.index;
          setHovered(detail.zone.item.id);
        } else {
          targetSlice = "core";
          setHovered(null);
        }
      } else {
        setHovered(null);
        const el = document.elementFromPoint(ev.clientX, ev.clientY);
        const slotChip = el?.closest("[data-slot-index]") as HTMLElement | null;
        if (slotChip && slotChip.dataset.slotIndex !== undefined) {
          targetSlice = Number(slotChip.dataset.slotIndex);
        } else if (el?.closest('[data-drop-zone="workbench"]')) {
          targetSlice = "core";
        }
      }

      setChipDrag({ item, fromSlotIndex, x: ev.clientX, y: ev.clientY, targetSlice });
    };

    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      setHovered(null);
      setChipDrag((last) => {
        if (!last || last.targetSlice === null) return null;
        const currentSlots = [...getSlotsForRing(activeWorkspace, activeEditRing)];

        if (last.targetSlice === "core") {
          if (!currentSlots.includes(item.id) && currentSlots.length < MAX_RING_SLOTS) {
            currentSlots.push(item.id);
            updateSlotsForRing(activeEditRing, currentSlots);
          }
        } else if (typeof last.targetSlice === "number") {
          const targetIdx = Math.min(last.targetSlice, currentSlots.length - 1);
          if (fromSlotIndex !== null) {
            // Reorder existing slot
            const [moved] = currentSlots.splice(fromSlotIndex, 1);
            currentSlots.splice(targetIdx, 0, moved);
            updateSlotsForRing(activeEditRing, currentSlots);
          } else {
            // Replace or move catalog item into targetIdx
            const existingIdx = currentSlots.indexOf(item.id);
            if (existingIdx >= 0 && existingIdx !== targetIdx) {
              const [moved] = currentSlots.splice(existingIdx, 1);
              currentSlots.splice(targetIdx, 0, moved);
            } else {
              currentSlots[targetIdx] = item.id;
            }
            updateSlotsForRing(activeEditRing, currentSlots);
          }
        }
        return null;
      });
    };

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
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
    const activeRings = ringsRef.current;
    let detail = zoneAt(point, center, activeRings[ringRef.current]);

    // Entering a navigation segment (category items, "More"/"Back") swaps the ring.
    // Only on entering, so the aligned segment under the cursor right after a swap
    // does not swap straight back.
    const moving = type === "over" || type === "move";
    if (moving && detail.zone.kind === "item" && detail.zone.index !== lastIndex.current) {
      const nav = detail.zone.item.nav;
      if (nav) {
        switchRing(nav);
        detail = zoneAt(point, center, activeRings[nav]);
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
    switchRing(entryRingRef.current);
    setDragging(false);
    setPicking(null);
    setHovered(null);
  };

  // Summary opens the panel beside the circle; its progress arrives as summary-progress
  const summarize = (paths: string[], toDownloads: boolean) => {
    window.clearTimeout(resetTimer.current);
    setStatus({ title: "Reading", hint: "Apple Intelligence", tone: "busy", ai: true });
    invoke("summarize", { paths, toDownloads }).catch((e) => {
      setStatus({ title: "Failed", hint: String(e), tone: "err" });
      resetTimer.current = window.setTimeout(() => setStatus(null), 3500);
    });
  };

  const runBatch = async (paths: string[], format: string, toDownloads = false, preset?: string | null) => {
    window.clearTimeout(resetTimer.current);
    const same = format === SAME_FORMAT;
    const presetLabel = preset && !same ? ` · ${labelOf(preset)}` : "";
    const label = same ? (labelOf(preset ?? "") ?? "Done") : (labelOf(format) ?? format.toUpperCase()) + presetLabel;
    const many = paths.length > 1;
    setStatus({
      title: many ? `0/${paths.length}` : format.startsWith("up-") ? "Upscaling" : format.startsWith("tr-") ? "Listening" : "Converting",
      hint: `→ ${label}`,
      tone: "busy",
      progress: many ? 0 : undefined,
    });

    const onProgress = new Channel<Progress>();
    onProgress.onmessage = ({ done, total, unit }) =>
      setStatus((s) => {
        if (s?.tone !== "busy") return s;
        // A single PDF reports its pages, a single upscale its steps; several files report files
        if (unit === "page" && total > 1) return { ...s, title: `Page ${done}/${total}`, progress: done / total };
        if (unit === "step" && total > 0) return { ...s, title: `${Math.floor((done / total) * 100)}%`, progress: done / total };
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
    const mergedGif = format === "gif-anim" && outputs.length === 1;
    const mergedSlides =
      format === "pptx" && many && outputs.length === 1 && paths.every((p) => !OFFICE_FILE.test(p) && !/\.pdf$/i.test(p));
    // Resizing/compressing in place: the size change is what the user wants to see
    const sizes =
      (same || preset === "compress" || format.startsWith("cmp-") || format === "vid-compress") &&
      bytes_before > 0;
    const done = sizes
      ? `${note ? `${note} · ` : bytes_after >= bytes_before ? "Already compact · " : ""}${formatBytes(bytes_before)} → ${formatBytes(bytes_after)}`
      : merged
      ? `${plural(paths.length - errors.length, "image")} → 1 PDF${presetLabel}`
      : mergedGif
      ? `${plural(paths.length - errors.length, "image")} → GIF${presetLabel}`
      : mergedSlides
      ? `${plural(paths.length - errors.length, "image")} → 1 presentation`
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
          setMergeable(p.paths.length > 1 && p.paths.every((path) => !OFFICE_FILE.test(path)));
          // It may be headed for Summary: the model loads while the user aims
          if (hasAiSupport() && p.paths.some(isDocument)) invoke("summary_warm").catch(() => {});
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
          if (format === AI_SUMMARY) summarize(p.paths, false);
          else if (format) runBatch(p.paths, format, false, chosen);
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
    if (!open || workshopOpen || !ALT_SUPPORTED) return;
    const timer = window.setInterval(async () => {
      const down = await invoke<boolean>("option_key_down").catch(() => false);
      if (down === altRef.current) return;
      altRef.current = down;
      if (down) ringBeforeAlt.current = ringRef.current === "size" ? entryRingRef.current : ringRef.current;
      switchRing(down ? "size" : ringBeforeAlt.current);
      lastIndex.current = null;
      // Re-evaluate where the cursor already is, so nothing waits for the next mouse move
      setHover(lastPoint.current ? resolve("over", lastPoint.current) : null);
    }, ALT_POLL_MS);
    return () => window.clearInterval(timer);
  }, [open, workshopOpen]);

  // "Ring Workshop..." from the tray menu
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    listen("toggle-workshop", () => {
      toggleWorkshop();
    }).then((fn) => (cancelled ? fn() : (unlisten = fn)));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // "Check for Updates..." from the tray menu + auto-check after launch
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    listen("check-update", () => {
      triggerUpdateCheck();
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });

    const timer = window.setTimeout(async () => {
      try {
        const up = await checkForUpdate();
        if (cancelled) return;
        if (up) {
          setUpdateInfo(up);
          setUpdateState({ status: "available", update: up, version: up.version, body: up.body });
        }
      } catch {
        // Quiet on startup background failure
      }
    }, 4000);

    return () => {
      cancelled = true;
      unlisten?.();
      window.clearTimeout(timer);
    };
  }, []);

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

  // Whether this computer can write AI summaries (Apple Intelligence)
  useEffect(() => {
    const known = (supported: boolean) => {
      setAiSupport(supported);
      setAiSupported(supported);
    };
    invoke<{ supported: boolean }>("summary_status")
      .then((a) => known(a.supported))
      .catch(() => known(false));
  }, []);

  // The summary panel's progress, shown by the circle as well
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    listen<SummaryProgress>("summary-progress", ({ payload }) => {
      window.clearTimeout(resetTimer.current);
      if (payload.tone === "idle") {
        setStatus((s) => (s?.ai ? null : s));
        return;
      }
      setStatus({
        title: payload.title,
        hint: payload.hint,
        tone: payload.tone,
        progress: payload.progress ?? undefined,
        ai: true,
      });
      if (payload.tone !== "busy") {
        resetTimer.current = window.setTimeout(() => setStatus(null), payload.tone === "ok" ? 8000 : 5000);
      }
    }).then((fn) => (cancelled ? fn() : (unlisten = fn)));
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
    switchRing(entryRingRef.current);
    lastIndex.current = null;
    if (pasted.kind === "files") {
      setFileCount(pasted.paths.length);
      setMergeable(pasted.paths.length > 1 && pasted.paths.every((path) => !OFFICE_FILE.test(path)));
      setPicking({ paths: pasted.paths, image: false });
    } else {
      setFileCount(1);
      setPicking({ paths: [pasted.path], image: true });
    }
  };

  // Cmd+Q quits, Cmd/Ctrl+V pastes, Cmd/Ctrl+, toggles Workshop, Esc cancels a paste or closes Workshop
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      const key = e.key.toLowerCase();
      if (mod && key === "q") {
        e.preventDefault();
        invoke("quit_app");
      } else if (mod && key === ",") {
        e.preventDefault();
        toggleWorkshop();
      } else if (mod && key === "v") {
        if ((e.target as HTMLElement)?.tagName === "INPUT") return;
        e.preventDefault();
        if (!draggingRef.current && statusRef.current?.tone !== "busy") paste();
      } else if (key === "escape") {
        if (workshopOpen) {
          toggleWorkshop(false);
        } else {
          closeRing();
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [workshopOpen]);

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
    if (status?.ai) {
      invoke("summary_show").catch(() => {});
      return;
    }
    const first = status?.outputs?.[0];
    if (first) revealItemInDir(first).catch(() => {});
  };

  const onCoreContextMenu = (e: MouseEvent) => {
    e.preventDefault();
    toggleWorkshop();
  };

  // While choosing a format for a paste, the mouse plays the role of the dragged file.
  // While in the Ring Workshop, moving/clicking on the ring previews slices and navigates sub-rings.
  const onPickMove = (e: MouseEvent) => {
    if (picking) {
      setHover(resolve("move", { x: e.clientX, y: e.clientY }));
    } else if (workshopOpen && !chipDrag) {
      const center = coreCenter();
      const pt = { x: e.clientX, y: e.clientY };
      if (Math.hypot(pt.x - center.x, pt.y - center.y) <= RING_OUTER + 18) {
        const detail = zoneAt(pt, center, ringsRef.current[ringRef.current]);
        setHovered(detail.zone.kind === "item" ? detail.zone.item.id : null);
      } else {
        setHovered(null);
      }
    }
  };
  const onPickClick = (e: MouseEvent) => {
    if (workshopOpen && !picking) {
      const center = coreCenter();
      const pt = { x: e.clientX, y: e.clientY };
      const dist = Math.hypot(pt.x - center.x, pt.y - center.y);
      if (dist > CORE_SIZE / 2 && dist <= RING_OUTER + 18) {
        const detail = zoneAt(pt, center, ringsRef.current[ringRef.current]);
        if (detail.zone.kind === "item" && detail.zone.item.nav && activeWorkspace.mode === "rooms") {
          const targetNav = detail.zone.item.nav;
          setEditingRing(targetNav);
          switchRing(targetNav);
        }
      }
      return;
    }
    if (!picking || coreRef.current?.contains(e.target as Node)) return;
    const format = resolve("click", { x: e.clientX, y: e.clientY });
    if (!format) return;
    const { paths, image } = picking;
    const chosen = presetRef.current;
    closeRing();
    if (format === AI_SUMMARY) summarize(paths, image);
    else runBatch(paths, format, image, chosen);
  };

  const hoveredLabel = hovered ? labelOf(hovered) : undefined;
  // The lobby names its rooms; an action placed in it (AI Summary) speaks for itself
  const hoveredAction = !!hovered && !built.rings[ringId].items.find((i) => i.id === hovered)?.nav;
  const inCategory = isCategoryRing(ringId) && !hoveredAction;

  let title = "Convert";
  let hint = "Drop files here";
  const presetLabel = preset ? labelOf(preset) : undefined;
  let badge: string | null =
    summary ?? (activeWorkspace.id !== "all-rooms" ? activeWorkspace.name : "Customize");
  if (workshopOpen) {
    if (chipDrag) {
      title = chipDrag.item.label;
      hint =
        chipDrag.targetSlice === "core"
          ? "+ Add to ring"
          : typeof chipDrag.targetSlice === "number"
            ? `Replace slot #${chipDrag.targetSlice + 1}`
            : "Drop on a slice";
      badge = "Workshop";
    } else if (hoveredLabel) {
      title = hoveredLabel;
      hint = "Drop to replace";
      badge = "Workshop";
    } else {
      title = activeWorkspace.mode === "direct" ? activeWorkspace.name : (CATEGORY_LABELS[ringId] ?? "Ring");
      hint = "Drop on a slice";
      badge = "Workshop";
    }
  } else if (open && ringId === "size") {
    title = presetLabel ?? "Size";
    hint = presetLabel
      ? `${dragging ? "Drop" : "Click"}: same format, or release ${ALT_KEY}`
      : "Point to a size";
    badge = null;
  } else if (open) {
    const verb = dragging ? "Release" : "Click";
    // Upscaling sets its own size, so size presets don't apply to it (nor to transcripts)
    const aiTool = hovered === AI_SUMMARY || hovered?.startsWith("up-") || hovered?.startsWith("tr-");
    badge = aiTool
      ? ON_DEVICE
      : presetLabel
        ? `Size: ${presetLabel}`
        : ALT_SUPPORTED
          ? `Hold ${ALT_KEY} to resize`
          : summary;
    if (inCategory) {
      // Category lobby — show room names
      title = "Convert";
      hint = "Pick a category";
    } else if (hoveredLabel) {
      title = hoveredLabel;
      hint =
        hovered === "clean"
          ? `${verb} to strip metadata`
          : hovered === "nobg"
            ? `${verb} to remove background`
          : hovered?.startsWith("up-")
            ? `${verb} to enlarge with AI`
          : hovered === "tr-srt" || hovered === "tr-vtt"
            ? `${verb} to make subtitles with AI`
          : hovered?.startsWith("tr-")
            ? `${verb} to write down what is said`
          : hovered === "square"
            ? `${verb} to pad to 1:1 square`
          : hovered === "gif-anim"
            ? `${verb} to combine into animated GIF`
          : hovered === "pptx" && fileCount > 1 && mergeable
            ? `${verb} to combine into 1 presentation`
          : hovered === "pptx"
            ? `${verb} to make PowerPoint slides`
          : hovered === "ico"
            ? `${verb} for multi-size .ico`
          : hovered === "icns"
            ? `${verb} for macOS .icns icon`
          : hovered?.startsWith("cmp-")
            ? `${verb} to compress file size`
          : hovered?.startsWith("rot-") || hovered?.startsWith("flip-") || hovered === "grayscale"
            ? `${verb} to transform photo`
          : hovered === "docx-ocr"
            ? `${verb} to read scanned text into Word`
          : hovered === AI_SUMMARY
            ? `${verb} to explain it with AI`
          : hovered === "vid-compress"
            ? `${verb} to shrink video file size`
          : hovered === "vid-mute"
            ? `${verb} to strip audio track`
          : hovered === "vid-mp3"
            ? `${verb} to extract audio track`
          : hovered === "vid-gif"
            ? `${verb} to convert video clip to GIF`
          : hovered === "vid-1080p" || hovered === "vid-720p"
            ? `${verb} to resize video resolution`
          : hovered === "sql"
            ? `${verb} to generate SQL table & inserts`
          : hovered === "md-table"
            ? `${verb} to export Markdown table`
          : hovered === "pdf" && fileCount > 1 && mergeable
            ? `${verb} to merge into 1 PDF`
            : `${verb} to convert`;
    } else {
      title = CATEGORY_LABELS[ringId] ?? (picking?.image ? "Image" : plural(fileCount, "file"));
      hint = dragging ? "Point to a format" : "Pick a format · Esc to cancel";
    }
  } else if (status) {
    title = status.title;
    hint = status.hint;
  }

  const tone = !open && status ? status.tone : "";
  const busy = !open && status?.tone === "busy";
  const progress = busy ? status?.progress : undefined;
  const clickable = !open && (!!status?.outputs?.length || !!status?.ai);
  const aiBusy = busy && !!status?.ai;

  return (
    <main
      className={[picking && hovered && "pick-ready", workshopOpen && "workshop-open"].filter(Boolean).join(" ")}
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
            {/* Pearl for the AI Summary slice: light, not colour */}
            <linearGradient id="seg-ai" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0" stopColor="#ffffff" />
              <stop offset="0.55" stopColor="#d6dfeb" />
              <stop offset="1" stopColor="#ffffff" />
            </linearGradient>
          </defs>

          {ringList.map((r) => (
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
                  r.id === "category" && "category-item",
                  item.id === AI_SUMMARY && "ai",
                ]
                  .filter(Boolean)
                  .join(" ");

                return (
                  <g key={`${item.id}-${i}`} className={cls} style={style}>
                    <g className="segment-inner">
                      <path className="segment-body" d={built.paths[r.id][i]} />
                      <path className="segment-rim" d={built.paths[r.id][i]} />
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

        {/* Drag the core to move the window, click it to reveal the result, or right-click for Ring Workshop */}
        <div
          ref={coreRef}
          className={`core glass ${tone} ${clickable ? "clickable" : ""} ${aiBusy ? "ai" : ""} ${
            chipDrag?.targetSlice === "core" ? "drop-core" : ""
          }`}
          onMouseDown={onCoreDown}
          onMouseMove={onCoreMove}
          onMouseUp={onCoreUp}
          onContextMenu={onCoreContextMenu}
          onMouseLeave={() => (press.current = null)}
        >
          {/* Apple Intelligence at work: a turning ring of its colours */}
          {aiBusy && (
            <>
              <span className="ai-halo-glow" aria-hidden="true" />
              <span className="ai-halo" aria-hidden="true" />
            </>
          )}
          {/* Determinate ring when progress is known, otherwise a spinning arc */}
          {busy && !aiBusy && (
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
          <span
            className={`core-title ${
              title.length > 10 ? "long" : title.length > 7 ? "medium" : ""
            }`}
          >
            {title}
          </span>
          <span className="core-hint">{hint}</span>
          {badge &&
            (!open ? (
              <button
                type="button"
                className="core-badge core-workshop-btn"
                onMouseDown={(e) => e.stopPropagation()}
                onClick={(e) => {
                  e.stopPropagation();
                  toggleWorkshop(true);
                }}
                title="Open Ring Workshop (Cmd+,)"
              >
                {badge}
              </button>
            ) : (
              <span className="core-badge">{badge}</span>
            ))}
        </div>
      </div>

      {workshopOpen && (
        <WorkshopPanel
          state={wsState}
          activeWorkspace={activeWorkspace}
          editingRing={editingRing}
          onSelectWorkspace={(id) => {
            const targetWs = wsState.workspaces.find((w) => w.id === id);
            updateWorkspaces((prev) => ({ ...prev, activeId: id }));
            if (targetWs) {
              switchRing(targetWs.mode === "direct" ? "custom" : editingRing);
            }
          }}
          onCreateWorkspace={(name) => {
            const id = `ws-${Date.now()}`;
            updateWorkspaces((prev) => ({
              activeId: id,
              workspaces: [
                ...prev.workspaces,
                {
                  id,
                  name,
                  mode: "direct",
                  customSlots: [...getSlotsForRing(activeWorkspace, "custom")],
                  roomSlots: {},
                },
              ],
            }));
            switchRing("custom");
          }}
          onDeleteWorkspace={(id) => {
            updateWorkspaces((prev) => {
              const filtered = prev.workspaces.filter((w) => w.id !== id);
              const nextActive = prev.activeId === id ? filtered[0].id : prev.activeId;
              return { activeId: nextActive, workspaces: filtered };
            });
          }}
          onSetMode={(mode) => {
            updateWorkspaces((prev) => ({
              ...prev,
              workspaces: prev.workspaces.map((ws) => (ws.id === prev.activeId ? { ...ws, mode } : ws)),
            }));
            switchRing(mode === "direct" ? "custom" : editingRing);
          }}
          onSelectEditingRing={(targetRing) => {
            setEditingRing(targetRing);
            switchRing(targetRing);
          }}
          onUpdateSlots={updateSlotsForRing}
          onResetRing={(targetRing) => {
            updateSlotsForRing(targetRing, defaultSlots(targetRing));
          }}
          onStartChipDrag={handleStartChipDrag}
          onWindowResized={() => sendGlass(GLASS_OPEN).catch(() => {})}
          onCheckUpdate={triggerUpdateCheck}
          onClose={() => toggleWorkshop(false)}
        />
      )}

      {chipDrag && (
        <div
          className="ws-drag-ghost glass"
          style={{ left: `${chipDrag.x}px`, top: `${chipDrag.y}px` }}
        >
          <span className="ws-drag-ghost-label">{chipDrag.item.label}</span>
          {chipDrag.item.sub && <span className="ws-drag-ghost-sub">{chipDrag.item.sub}</span>}
        </div>
      )}

      <UpdateDialog
        update={updateInfo}
        state={updateState}
        toastMessage={toastMessage}
        onDismiss={() => {
          setUpdateInfo(null);
          setUpdateState({ status: "idle" });
        }}
        onInstallStart={() => setUpdateState({ status: "downloading", progress: 0 })}
        onProgress={(progress, total) => setUpdateState({ status: "downloading", progress, total })}
        onError={(msg) => {
          setUpdateState({ status: "error", message: msg });
          showToast(`Update error: ${msg}`);
          setUpdateInfo(null);
        }}
      />

      {DEBUG_DROP && <DebugOverlay info={debugInfo} />}
    </main>
  );
}
