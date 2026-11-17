import type { RingId } from "../rings/dropTarget";
import CatalogBrowser from "./components/CatalogBrowser";
import ModeBar from "./components/ModeBar";
import SlotWorkbench from "./components/SlotWorkbench";
import WorkshopHeader from "./components/WorkshopHeader";
import WorkspacePicker from "./components/WorkspacePicker";
import { useChipPress } from "./hooks/useChipPress";
import { useWindowResize } from "./hooks/useWindowResize";
import { isDefaultLayout, removeSlot, toggleSlot } from "./slots";
import {
  EDITABLE_RINGS,
  editableRing,
  getSlotsForRing,
  type CatalogEntry,
  type WorkspaceConfig,
  type WorkspacesState,
} from "./workshop";
import "./styles/index.css";

type Props = {
  state: WorkspacesState;
  activeWorkspace: WorkspaceConfig;
  editingRing: RingId;
  onSelectWorkspace: (id: string) => void;
  onCreateWorkspace: (name: string) => void;
  onDeleteWorkspace: (id: string) => void;
  onSetMode: (mode: WorkspaceConfig["mode"]) => void;
  onSelectEditingRing: (ringId: RingId) => void;
  onUpdateSlots: (ringId: RingId, slots: string[]) => void;
  onResetRing: (ringId: RingId) => void;
  onStartChipDrag: (item: CatalogEntry, fromSlotIndex: number | null, clientX: number, clientY: number) => void;
  onWindowResized: () => void;
  onCheckUpdate?: () => void;
  onClose: () => void;
};

/** The Ring Workshop, beside the circle: workspaces, the ring's slots, and the catalog */
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
  onCheckUpdate,
  onClose,
}: Props) {
  const ring = editableRing(activeWorkspace.mode, editingRing);
  const slots = getSlotsForRing(activeWorkspace, ring);
  const resize = useWindowResize(onWindowResized);

  const updateSlots = (next: string[] | null) => next && onUpdateSlots(ring, next);
  const pressChip = useChipPress({
    onDragStart: onStartChipDrag,
    onClick: (item) => updateSlots(toggleSlot(slots, item.id)),
  });

  const ringName =
    activeWorkspace.mode === "direct"
      ? `Main Ring (${activeWorkspace.name})`
      : (EDITABLE_RINGS.find((r) => r.id === editingRing)?.label ?? editingRing);

  return (
    <section className="workshop glass" aria-label="Ring Workshop">
      <WorkshopHeader
        sizePreset={resize.preset}
        onSizePreset={resize.applyPreset}
        onCheckUpdate={onCheckUpdate}
        onClose={onClose}
      />

      {/* Single smooth scrollable body so nothing is ever cut off on smaller window sizes */}
      <div className="ws-scroll-body">
        <div className="ws-top-controls">
          <WorkspacePicker
            state={state}
            activeId={activeWorkspace.id}
            onSelect={onSelectWorkspace}
            onCreate={onCreateWorkspace}
            onDelete={onDeleteWorkspace}
          />
          <ModeBar
            mode={activeWorkspace.mode}
            editingRing={editingRing}
            onSetMode={onSetMode}
            onSelectEditingRing={onSelectEditingRing}
          />
        </div>

        <SlotWorkbench
          ringName={ringName}
          slots={slots}
          modified={!isDefaultLayout(ring, slots)}
          onReset={() => onResetRing(ring)}
          onRemove={(index) => updateSlots(removeSlot(slots, index))}
          onPressChip={pressChip}
        />

        <CatalogBrowser ring={ring} slots={slots} onPressChip={pressChip} />
      </div>

      <div className="ws-resize-grip" onPointerDown={resize.onGripDown} title="Drag corner to resize window">
        <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
          <path d="M12 3L3 12M12 7L7 12M12 11L11 12" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
        </svg>
      </div>
    </section>
  );
}
