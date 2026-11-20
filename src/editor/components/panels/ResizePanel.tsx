import type { FocusEvent, KeyboardEvent } from "react";
import { cls } from "../../../shared/cls";
import { outputSize, type Edits, type Size } from "../../lib/pipeline";

type Props = {
  original: Size;
  edits: Edits;
  onScale: (scale: number) => void;
};

const PERCENTS = [1, 0.75, 0.5, 0.25];
const LONG_EDGES = [3840, 2560, 1920, 1280, 1080];

/** Saving smaller: an exact width or height, a percent, or the longest side (never enlarged) */
export default function ResizePanel({ original, edits, onScale }: Props) {
  const full = outputSize(original, { ...edits, scale: 1 });
  const out = outputSize(original, edits);
  const longest = Math.max(full.width, full.height);
  const scaleTo = (side: number, of: number) => onScale(Math.min(1, Math.max(0.01, side / of)));

  // Typed sizes apply on Enter or when the field is left; never larger than the photo
  const typed = (of: number, current: number) => ({
    onBlur: (e: FocusEvent<HTMLInputElement>) => {
      const v = Math.min(Math.round(Number(e.target.value)), of);
      if (!(v > 0)) {
        e.target.value = String(current);
        return;
      }
      // (shown at once: the field is only rebuilt when the saved size changes)
      e.target.value = String(v);
      scaleTo(v, of);
    },
    onKeyDown: (e: KeyboardEvent<HTMLInputElement>) => {
      if (e.key === "Enter") e.currentTarget.blur();
    },
  });

  return (
    <div className="ed-resize">
      <div className="ed-size-fields">
        <input
          key={`w${out.width}`}
          type="number"
          min={1}
          max={full.width}
          defaultValue={out.width}
          aria-label="Width"
          {...typed(full.width, out.width)}
        />
        <span className="ed-size-x">×</span>
        <input
          key={`h${out.height}`}
          type="number"
          min={1}
          max={full.height}
          defaultValue={out.height}
          aria-label="Height"
          {...typed(full.height, out.height)}
        />
        <span className="ed-size-unit">px</span>
      </div>
      <div className="ed-chips">
        {PERCENTS.map((p) => (
          <button key={p} type="button" className={cls("ed-chip", edits.scale === p && "active")} onClick={() => onScale(p)}>
            {p * 100}%
          </button>
        ))}
        <span className="ed-chips-sep" />
        {LONG_EDGES.map((edge) => (
          <button
            key={edge}
            type="button"
            className={cls("ed-chip", Math.max(out.width, out.height) === edge && "active")}
            disabled={edge >= longest}
            onClick={() => scaleTo(edge, longest)}
            title={`Longest side ${edge} px`}
          >
            {edge}
          </button>
        ))}
      </div>
    </div>
  );
}
