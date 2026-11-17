import type { MouseEvent } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { cls } from "../../shared/cls";
import { WINDOW_PRESETS } from "../workshopWindow";

type Props = {
  sizePreset: string;
  onSizePreset: (id: string) => void;
  onCheckUpdate?: () => void;
  onClose: () => void;
};

// Pressing the header (not its controls) moves the borderless window
const onHeaderMouseDown = (e: MouseEvent) => {
  if (e.button === 0 && (e.target as HTMLElement).closest("button, input") === null) {
    getCurrentWindow().startDragging();
  }
};

/** Pinned top header: title, window size presets, Updates and Done */
export default function WorkshopHeader({ sizePreset, onSizePreset, onCheckUpdate, onClose }: Props) {
  return (
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
              className={cls("ws-size-btn", sizePreset === p.id && "active")}
              onClick={() => onSizePreset(p.id)}
              title={p.title}
            >
              {p.label}
            </button>
          ))}
        </div>
        {onCheckUpdate && (
          <button type="button" className="ws-btn ws-btn-ghost" onClick={onCheckUpdate} title="Check for application updates">
            Updates
          </button>
        )}
        <button type="button" className="ws-btn ws-btn-primary" onClick={onClose}>
          Done
        </button>
      </div>
    </header>
  );
}
