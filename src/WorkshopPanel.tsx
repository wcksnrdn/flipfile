import { useState, type MouseEvent, type PointerEvent as ReactPointerEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import type { RingId } from "./dropTarget";
import {
  CATALOG,
  CATALOG_CATEGORIES,
  DEFAULT_ROOM_SLOTS,
  EDITABLE_RINGS,
  getCatalogItem,
  getSlotsForRing,
  type CatalogCategory,
  type CatalogEntry,
  type WorkspaceConfig,
  type WorkspacesState,
} from "./workshop";

export const MIN_RING_SLOTS = 2;
export const MAX_RING_SLOTS = 10;

const WINDOW_PRESETS = [
  { id: "compact", label: "S", title: "Compact Window (820×540)", w: 820, h: 540 },
  { id: "comfort", label: "M", title: "Default Window (940×620)", w: 940, h: 620 },
  { id: "large", label: "L", title: "Large Window (1120×720)", w: 1120, h: 720 },
] as const;

type Props = {
  state: WorkspacesState;
  activeWorkspace: WorkspaceConfig;
  editingRing: RingId;
  onSelectWorkspace: (id: string) => void;
  onCreateWorkspace: (name: string) => void;
  onDeleteWorkspace: (id: string) => void;
  onSetMode: (mode: "rooms" | "direct") => void;
  onSelectEditingRing: (ringId: RingId) => void;
  onUpdateSlots: (ringId: RingId, slots: string[]) => void;
  onResetRing: (ringId: RingId) => void;
  onStartChipDrag: (item: CatalogEntry, fromSlotIndex: number | null, clientX: number, clientY: number) => void;
  onWindowResized: () => void;
  onClose: () => void;
};

export default function WorkshopPanel({
  state,
  activeWorkspace,
  editingRing,
  onSelectWorkspace,
  onCreateWorkspace,
  onDeleteWorkspace,
  onSetMode,
  onSelectEditingRing,
  onUpdateSlots,
  onResetRing,
  onStartChipDrag,
  onWindowResized,
  onClose,
}: Props) {
  const [category, setCategory] = useState<"all" | CatalogCategory>("all");
  const [query, setQuery] = useState("");
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const [sizePreset, setSizePreset] = useState<string>("comfort");

  const currentRingId: RingId = activeWorkspace.mode === "direct" ? "custom" : editingRing;
  const slots = getSlotsForRing(activeWorkspace, currentRingId);

  const applyWindowSize = (w: number, h: number, presetId?: string) => {
    if (presetId) setSizePreset(presetId);
    const cw = Math.round(Math.max(720, Math.min(1800, w)));
    const ch = Math.round(Math.max(480, Math.min(1200, h)));
    invoke("set_workshop_window", { open: true, width: cw, height: ch }).catch(() => {});
    getCurrentWindow()
      .setSize(new LogicalSize(cw, ch))
      .then(onWindowResized)
      .catch(() => {});
    window.setTimeout(onWindowResized, 60);
  };

  // Drag the bottom-right corner grip to resize the borderless window smoothly
  const onResizeGripDown = (e: ReactPointerEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.screenX;
    const startY = e.screenY;
    const startW = window.innerWidth;
    const startH = window.innerHeight;
    setSizePreset("");

    let raf = 0;
    const onMove = (ev: PointerEvent) => {
      const nextW = startW + (ev.screenX - startX);
      const nextH = startH + (ev.screenY - startY);
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => {
        applyWindowSize(nextW, nextH);
      });
    };

    const onUp = () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      onWindowResized();
    };

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  const toggleCatalogItem = (item: CatalogEntry) => {
    const idx = slots.indexOf(item.id);
    if (idx >= 0) {
      if (slots.length > MIN_RING_SLOTS) {
        onUpdateSlots(
          currentRingId,
          slots.filter((id) => id !== item.id)
        );
      }
    } else if (slots.length < MAX_RING_SLOTS) {
      onUpdateSlots(currentRingId, [...slots, item.id]);
    }
  };

  const removeSlotAt = (index: number) => {
    if (slots.length <= MIN_RING_SLOTS) return;
    const next = [...slots];
    next.splice(index, 1);
    onUpdateSlots(currentRingId, next);
  };

  const handleChipPointerDown = (
    e: ReactPointerEvent,
    item: CatalogEntry,
    fromSlotIndex: number | null
  ) => {
    if (e.button !== 0) return;
    const startX = e.clientX;
    const startY = e.clientY;
    let moved = false;

    const onMove = (ev: PointerEvent) => {
      if (!moved && Math.hypot(ev.clientX - startX, ev.clientY - startY) > 5) {
        moved = true;
        onStartChipDrag(item, fromSlotIndex, ev.clientX, ev.clientY);
      }
    };

    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      if (!moved && fromSlotIndex === null) {
        toggleCatalogItem(item);
      }
    };

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  const submitCreate = () => {
    const trimmed = newName.trim();
    if (trimmed) {
      onCreateWorkspace(trimmed);
      setNewName("");
      setCreating(false);
    }
  };

  const onHeaderMouseDown = (e: MouseEvent) => {
    if (e.button === 0 && (e.target as HTMLElement).closest("button, input") === null) {
      getCurrentWindow().startDragging();
    }
  };

  const filteredCatalog = CATALOG.filter((item) => {
    if (item.nav === currentRingId) return false;
    if (category !== "all" && item.category !== category) return false;
    if (query.trim()) {
      const q = query.trim().toLowerCase();
      return (
        item.label.toLowerCase().includes(q) ||
        item.title.toLowerCase().includes(q) ||
        item.id.toLowerCase().includes(q) ||
        (item.sub && item.sub.toLowerCase().includes(q))
      );
    }
    return true;
  });

  const defaultSlots = DEFAULT_ROOM_SLOTS[currentRingId] ?? [];
  const isModified =
    slots.length !== defaultSlots.length || slots.some((id, idx) => id !== defaultSlots[idx]);

  const currentRingName =
    activeWorkspace.mode === "direct"
      ? `Main Ring (${activeWorkspace.name})`
      : (EDITABLE_RINGS.find((r) => r.id === editingRing)?.label ?? editingRing);

  return (
    <section className="workshop glass" aria-label="Ring Workshop">
      {/* Pinned Top Header (draggable to move window) */}
      <header className="ws-header" onMouseDown={onHeaderMouseDown}>
        <div className="ws-title-group">
          <div className="ws-title-row">
            <h2 className="ws-title">Ring Workshop</h2>
            <span className="ws-badge-pill">Customize</span>
          </div>
          <span className="ws-subtitle">
            Click formats below to add or remove, or drag them onto the ring slices on the left.
          </span>
        </div>

        <div className="ws-header-actions">
          <div className="ws-size-group" title="Window size presets">
            {WINDOW_PRESETS.map((p) => (
              <button
                key={p.id}
                type="button"
                className={`ws-size-btn ${sizePreset === p.id ? "active" : ""}`}
                onClick={() => applyWindowSize(p.w, p.h, p.id)}
                title={p.title}
              >
                {p.label}
              </button>
            ))}
          </div>
          <button type="button" className="ws-btn ws-btn-primary" onClick={onClose}>
            Done
          </button>
        </div>
      </header>

      {/* Single smooth scrollable body so nothing is ever cut off on smaller window sizes */}
      <div className="ws-scroll-body">
        {/* Step 1: Workspace & Ring Mode */}
        <div className="ws-top-controls">
          <div className="ws-workspaces">
            <div className="ws-row-header">
              <span className="ws-section-label">1. Workspace Preset</span>
              <span className="ws-helper-text">Switch profiles for different workflows</span>
            </div>
            <div className="ws-pill-row">
              {state.workspaces.map((ws) => {
                const active = ws.id === activeWorkspace.id;
                return (
                  <div key={ws.id} className={`ws-preset-pill ${active ? "active" : ""}`}>
                    <button
                      type="button"
                      className="ws-preset-select"
                      onClick={() => onSelectWorkspace(ws.id)}
                    >
                      {ws.name}
                    </button>
                    {!ws.builtin && state.workspaces.length > 1 && (
                      <button
                        type="button"
                        className="ws-preset-del"
                        onClick={() => onDeleteWorkspace(ws.id)}
                        title="Delete workspace"
                      >
                        ×
                      </button>
                    )}
                  </div>
                );
              })}
              {creating ? (
                <div className="ws-new-form">
                  <input
                    type="text"
                    className="ws-new-input"
                    placeholder="Workspace name..."
                    value={newName}
                    autoFocus
                    maxLength={22}
                    onChange={(e) => setNewName(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") submitCreate();
                      if (e.key === "Escape") setCreating(false);
                    }}
                  />
                  <button type="button" className="ws-btn ws-btn-primary ws-btn-sm" onClick={submitCreate}>
                    Save
                  </button>
                  <button
                    type="button"
                    className="ws-btn ws-btn-ghost ws-btn-sm"
                    onClick={() => setCreating(false)}
                  >
                    Cancel
                  </button>
                </div>
              ) : (
                <button
                  type="button"
                  className="ws-add-ws-btn"
                  onClick={() => setCreating(true)}
                  title="Create a new custom workspace"
                >
                  + New Workspace
                </button>
              )}
            </div>
          </div>

          {/* Mode Switcher: Direct 1-Ring vs 4 Category Rooms */}
          <div className="ws-mode-bar">
            <div className="ws-mode-cards" role="tablist">
              <button
                type="button"
                className={`ws-mode-card ${activeWorkspace.mode === "direct" ? "active" : ""}`}
                onClick={() => onSetMode("direct")}
              >
                <span className="ws-mode-card-title">Direct Ring (1-Step)</span>
                <span className="ws-mode-card-desc">
                  Open your favorite formats on a single ring right away
                </span>
              </button>
              <button
                type="button"
                className={`ws-mode-card ${activeWorkspace.mode === "rooms" ? "active" : ""}`}
                onClick={() => onSetMode("rooms")}
              >
                <span className="ws-mode-card-title">Category Rooms</span>
                <span className="ws-mode-card-desc">
                  Start with Photo, Office, Video, and Music rooms
                </span>
              </button>
            </div>

            {activeWorkspace.mode === "rooms" && (
              <div className="ws-ring-tabs-wrap">
                <span className="ws-ring-tabs-label">Editing Ring:</span>
                <div className="ws-ring-tabs">
                  {EDITABLE_RINGS.map((r) => (
                    <button
                      key={r.id}
                      type="button"
                      className={`ws-ring-tab ${editingRing === r.id ? "active" : ""}`}
                      onClick={() => onSelectEditingRing(r.id)}
                    >
                      {r.label}
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Step 2: Active Slots Workbench */}
        <div className="ws-workbench" data-drop-zone="workbench">
          <div className="ws-workbench-head">
            <div className="ws-workbench-title-group">
              <span className="ws-section-label">
                2. Active Slots — {currentRingName} ({slots.length}/{MAX_RING_SLOTS})
              </span>
              <span className="ws-workbench-hint">
                {slots.length <= MIN_RING_SLOTS
                  ? "Minimum 2 slots required"
                  : slots.length >= MAX_RING_SLOTS
                    ? "Maximum 10 slots reached — drag onto a ring slice to replace"
                    : "Drag slots to reorder, or click × to keep your ring clean"}
              </span>
            </div>
            {isModified && (
              <button
                type="button"
                className="ws-btn ws-btn-ghost ws-btn-sm"
                onClick={() => onResetRing(currentRingId)}
                title="Reset this ring to its default formats"
              >
                Reset Default
              </button>
            )}
          </div>

          <div className="ws-slot-list">
            {slots.map((id, idx) => {
              const entry = getCatalogItem(id);
              if (!entry) return null;
              return (
                <div
                  key={`${id}-${idx}`}
                  className="ws-slot-chip"
                  data-slot-index={idx}
                  onPointerDown={(e) => handleChipPointerDown(e, entry, idx)}
                  title={`${entry.title} — drag to reorder, or click × to remove`}
                >
                  <span className="ws-slot-num">{idx + 1}</span>
                  <span className="ws-slot-label">
                    {entry.label}
                    {entry.sub && <small> {entry.sub}</small>}
                  </span>
                  {slots.length > MIN_RING_SLOTS && (
                    <button
                      type="button"
                      className="ws-slot-remove"
                      onPointerDown={(e) => e.stopPropagation()}
                      onClick={() => removeSlotAt(idx)}
                      aria-label={`Remove ${entry.label}`}
                    >
                      ×
                    </button>
                  )}
                </div>
              );
            })}
          </div>
        </div>

        {/* Step 3: Catalog of 75+ Formats & Tools */}
        <div className="ws-catalog">
          <div className="ws-catalog-head">
            <span className="ws-section-label">
              3. Format & Tool Catalog ({filteredCatalog.length})
            </span>
            <input
              type="search"
              className="ws-search"
              placeholder="Search formats & tools..."
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>

          <div className="ws-cat-tabs">
            {CATALOG_CATEGORIES.map((c) => (
              <button
                key={c.id}
                type="button"
                className={`ws-cat-tab ${category === c.id ? "active" : ""}`}
                onClick={() => setCategory(c.id)}
              >
                {c.label}
              </button>
            ))}
          </div>

          <div className="ws-catalog-grid">
            {filteredCatalog.map((item) => {
              const inRing = slots.includes(item.id);
              return (
                <div
                  key={item.id}
                  className={`ws-cat-chip ${inRing ? "selected" : ""} ${item.disabled ? "disabled" : ""}`}
                  onPointerDown={(e) => handleChipPointerDown(e, item, null)}
                  title={
                    inRing
                      ? `${item.title} — Click to remove from ring, or drag to a slot`
                      : `${item.title} — Click to add to ring, or drag onto a slice on the left`
                  }
                >
                  <div className="ws-cat-chip-top">
                    <span className="ws-cat-chip-label">{item.label}</span>
                    {item.sub && <span className="ws-cat-chip-sub">{item.sub}</span>}
                    <span className={`ws-cat-chip-action ${inRing ? "on" : "off"}`}>
                      {inRing ? "✓ Added" : "+ Add"}
                    </span>
                  </div>
                  <span className="ws-cat-chip-title">{item.title}</span>
                </div>
              );
            })}
          </div>
        </div>
      </div>

      {/* Draggable bottom-right resize grip */}
      <div
        className="ws-resize-grip"
        onPointerDown={onResizeGripDown}
        title="Drag corner to resize window"
      >
        <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
          <path
            d="M12 3L3 12M12 7L7 12M12 11L11 12"
            stroke="currentColor"
            strokeWidth="1.6"
            strokeLinecap="round"
          />
        </svg>
      </div>
    </section>
  );
}
