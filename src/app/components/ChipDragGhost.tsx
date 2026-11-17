import type { ChipDrag } from "../lib/types";

/** The Workshop chip following the pointer while it is dragged */
export default function ChipDragGhost({ drag }: { drag: ChipDrag }) {
  return (
    <div className="ws-drag-ghost glass" style={{ left: `${drag.x}px`, top: `${drag.y}px` }}>
      <span className="ws-drag-ghost-label">{drag.item.label}</span>
      {drag.item.sub && <span className="ws-drag-ghost-sub">{drag.item.sub}</span>}
    </div>
  );
}
