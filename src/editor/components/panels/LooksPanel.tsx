import { useEffect, useMemo, useRef } from "react";
import { cls } from "../../../shared/cls";
import { LOOKS, lookOf, type Look } from "../../lib/looks";
import type { Adjust, Edits } from "../../lib/pipeline";
import { drawEdited, thumbnailPixels, tonedCanvas } from "../../lib/render";

type Props = {
  pixels: ImageData;
  edits: Edits;
  onPick: (adjust: Adjust) => void;
};

/** Longest side of a look's thumbnail, in pixels (about twice its size on screen) */
const THUMB_MAX = 140;

/** One-tap looks in a row, each shown on the photo itself */
export default function LooksPanel({ pixels, edits, onPick }: Props) {
  const thumb = useMemo(() => thumbnailPixels(pixels, THUMB_MAX), [pixels]);
  const current = lookOf(edits.adjust);

  return (
    <div className="ed-looks">
      {LOOKS.map((look) => (
        <button
          key={look.id}
          type="button"
          className={cls("ed-look", current?.id === look.id && "active")}
          onClick={() => onPick(look.adjust)}
        >
          <LookThumb thumb={thumb} edits={edits} look={look} />
          <span>{look.label}</span>
        </button>
      ))}
    </div>
  );
}

function LookThumb({ thumb, edits, look }: { thumb: ImageData; edits: Edits; look: Look }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const toned = useMemo(() => tonedCanvas(thumb, look.adjust), [thumb, look]);
  useEffect(() => {
    if (ref.current) drawEdited(ref.current, toned, edits, false);
  }, [toned, edits]);
  return <canvas ref={ref} className="ed-look-thumb" />;
}
