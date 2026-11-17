import { getCatalogItem, MAX_RING_SLOTS, MIN_RING_SLOTS } from "../workshop";
import type { PressChip } from "../hooks/useChipPress";

type Props = {
  ringName: string;
  slots: string[];
  modified: boolean;
  onReset: () => void;
  onRemove: (index: number) => void;
  onPressChip: PressChip;
};

const hintFor = (count: number) =>
  count <= MIN_RING_SLOTS
    ? `Minimum ${MIN_RING_SLOTS} slots required`
    : count >= MAX_RING_SLOTS
      ? `Maximum ${MAX_RING_SLOTS} slots reached — drag onto a ring slice to replace`
      : "Drag slots to reorder, or click × to keep your ring clean";

/** Step 2: the ring's slots in order. Also a drop zone for chips (see useChipDrag) */
export default function SlotWorkbench({ ringName, slots, modified, onReset, onRemove, onPressChip }: Props) {
  const removable = slots.length > MIN_RING_SLOTS;
  return (
    <div className="ws-workbench" data-drop-zone="workbench">
      <div className="ws-workbench-head">
        <div className="ws-workbench-title-group">
          <span className="ws-section-label">
            2. Active Slots — {ringName} ({slots.length}/{MAX_RING_SLOTS})
          </span>
          <span className="ws-workbench-hint">{hintFor(slots.length)}</span>
        </div>
        {modified && (
          <button
            type="button"
            className="ws-btn ws-btn-ghost ws-btn-sm"
            onClick={onReset}
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
              onPointerDown={(e) => onPressChip(e, entry, idx)}
              title={`${entry.title} — drag to reorder, or click × to remove`}
            >
              <span className="ws-slot-num">{idx + 1}</span>
              <span className="ws-slot-label">
                {entry.label}
                {entry.sub && <small> {entry.sub}</small>}
              </span>
              {removable && (
                <button
                  type="button"
                  className="ws-slot-remove"
                  onPointerDown={(e) => e.stopPropagation()}
                  onClick={() => onRemove(idx)}
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
  );
}
