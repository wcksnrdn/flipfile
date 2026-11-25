import { useState } from "react";
import type { RingId } from "../../rings/dropTarget";
import { packNeeded, sizeLabel } from "../../packs/packs";
import { cls } from "../../shared/cls";
import type { PressChip } from "../hooks/useChipPress";
import { filterCatalog } from "../slots";
import { CATALOG_CATEGORIES, type CatalogCategory } from "../workshop";

type Props = {
  ring: RingId;
  slots: string[];
  onPressChip: PressChip;
};

/** Step 3: every format and tool, filtered by category and search */
export default function CatalogBrowser({ ring, slots, onPressChip }: Props) {
  const [category, setCategory] = useState<"all" | CatalogCategory>("all");
  const [query, setQuery] = useState("");
  const items = filterCatalog(ring, category, query);

  return (
    <div className="ws-catalog">
      <div className="ws-catalog-head">
        <span className="ws-section-label">3. Format & Tool Catalog ({items.length})</span>
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
            className={cls("ws-cat-tab", category === c.id && "active")}
            onClick={() => setCategory(c.id)}
          >
            {c.label}
          </button>
        ))}
      </div>

      <div className="ws-catalog-grid">
        {items.map((item) => {
          const inRing = slots.includes(item.id);
          const pack = item.disabled ? undefined : packNeeded(item.id, item.nav);
          return (
            <div
              key={item.id}
              className={cls("ws-cat-chip", inRing && "selected", item.disabled && "disabled")}
              onPointerDown={(e) => onPressChip(e, item, null)}
              title={
                inRing
                  ? `${item.title} — Click to remove from ring, or drag to a slot`
                  : `${item.title} — Click to add to ring, or drag onto a slice on the left`
              }
            >
              <div className="ws-cat-chip-top">
                <span className="ws-cat-chip-label">{item.label}</span>
                {item.sub && <span className="ws-cat-chip-sub">{item.sub}</span>}
                <span className={`ws-cat-chip-action ${inRing ? "on" : "off"}`}>{inRing ? "✓ Added" : "+ Add"}</span>
              </div>
              <span className="ws-cat-chip-title">{item.title}</span>
              {pack && <span className="ws-cat-chip-pack">Needs {pack.name} · {sizeLabel(pack.download || pack.size)} (above)</span>}
            </div>
          );
        })}
      </div>
    </div>
  );
}
