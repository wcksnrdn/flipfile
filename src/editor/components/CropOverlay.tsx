import { useEffect, useRef, type PointerEvent as ReactPointerEvent, type RefObject } from "react";
import { dragCrop, FULL_CROP, type Crop, type Handle } from "../lib/pipeline";

type Props = {
  crop: Crop | null;
  /** A locked shape, in fractions of the frame (fitAspect); null is free */
  ratio: number | null;
  /** The photo on screen, which the crop's fractions are of */
  frameRef: RefObject<HTMLDivElement | null>;
  /** Live while dragging, then committed once when let go (null keeps the whole photo) */
  onChange: (crop: Crop | null, commit: boolean) => void;
};

const HANDLES: Handle[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

/** Close enough to the whole photo to keep all of it */
const isWhole = (c: Crop) => c.x < 1e-4 && c.y < 1e-4 && c.width > 1 - 1e-4 && c.height > 1 - 1e-4;

const pct = (v: number) => `${v * 100}%`;

/** The crop box over the photo: drag inside to move it, the handles to resize it */
export default function CropOverlay({ crop, ratio, frameRef, onChange }: Props) {
  const box = crop ?? FULL_CROP;
  const stopDrag = useRef<(() => void) | null>(null);
  useEffect(() => () => stopDrag.current?.(), []);

  const startDrag = (e: ReactPointerEvent, handle: Handle) => {
    const frame = frameRef.current?.getBoundingClientRect();
    if (e.button !== 0 || !frame) return;
    e.preventDefault();
    e.stopPropagation();
    const from = { x: e.clientX, y: e.clientY };
    let last = box;

    const onMove = (ev: PointerEvent) => {
      last = dragCrop(box, handle, (ev.clientX - from.x) / frame.width, (ev.clientY - from.y) / frame.height, ratio);
      onChange(last, false);
    };
    const stop = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      stopDrag.current = null;
    };
    const onUp = () => {
      stop();
      onChange(isWhole(last) ? null : last, true);
    };
    stopDrag.current?.();
    stopDrag.current = stop;
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  const place = { left: pct(box.x), top: pct(box.y), width: pct(box.width), height: pct(box.height) };

  return (
    <div className="ed-crop">
      {/* The dimming is clipped to the photo; the handles are not, so they can be grabbed at its edges */}
      <div className="ed-crop-shade" aria-hidden="true">
        <span style={place} />
      </div>
      <div className="ed-crop-box" style={place} onPointerDown={(e) => startDrag(e, "move")}>
        <span className="ed-crop-thirds" aria-hidden="true" />
        {HANDLES.map((h) => (
          <span key={h} className={`ed-crop-handle ${h}`} onPointerDown={(e) => startDrag(e, h)} />
        ))}
      </div>
    </div>
  );
}
