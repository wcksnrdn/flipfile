import { useDeferredValue, useEffect, useMemo, useRef } from "react";
import { cls } from "../../shared/cls";
import { useElementSize } from "../hooks/useElementSize";
import { isIdentityAdjust, NO_ADJUST, outputSize, type Crop, type Edits, type Size } from "../lib/pipeline";
import { drawEdited, shownSize, tonedCanvas } from "../lib/render";
import CropOverlay from "./CropOverlay";

type Props = {
  pixels: ImageData;
  /** The photo's own size, which the saved size is worked out from */
  original: Size;
  edits: Edits;
  /** Held down: the photo without its adjustments */
  comparing: boolean;
  /** The crop tool is open: the whole frame is shown, with the crop box on top */
  cropping: boolean;
  /** The resize tool is open: the photo is shown at its new size beside its old one */
  resizing: boolean;
  /** The crop's locked shape in fractions of the frame, or null */
  cropRatio: number | null;
  onCrop: (crop: Crop | null, commit: boolean) => void;
};

/** Room around the photo inside its area, so the crop handles can be grabbed at the edges */
const MARGIN = 18;
/** Room under the photo for its size while resizing */
const SIZE_LABEL_ROOM = 22;
/** Width of the photo's glow behind it: tiny, it is blurred away anyway */
const AMBIENT_WIDTH = 48;
/** A photo resized very small is still shown at least this share of its outline (its label says how small) */
const MIN_SHOWN_SCALE = 0.2;

const dims = (s: Size) => `${s.width} × ${s.height}`;

/** The photo as edited, fitted to its area */
export default function PhotoCanvas({ pixels, original, edits, comparing, cropping, resizing, cropRatio, onCrop }: Props) {
  const areaRef = useRef<HTMLDivElement>(null);
  const frameRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const ambientRef = useRef<HTMLCanvasElement>(null);
  const area = useElementSize(areaRef);

  // Toning every pixel takes a few frames on a large preview: while a slider moves, the
  // slider stays smooth and the photo follows as soon as it can
  const adjust = useDeferredValue(edits.adjust);
  const plain = useMemo(() => tonedCanvas(pixels, NO_ADJUST), [pixels]);
  const toned = useMemo(() => (isIdentityAdjust(adjust) ? plain : tonedCanvas(pixels, adjust)), [pixels, plain, adjust]);
  const source = comparing ? plain : toned;

  // What will be saved, and its size before resizing (the crop's)
  const output = outputSize(original, edits);
  const unresized = outputSize(original, { ...edits, scale: 1 });

  useEffect(() => {
    // Never sharper than the saved photo (only while the crop is chosen is it the whole frame)
    if (canvasRef.current) drawEdited(canvasRef.current, source, edits, cropping, cropping ? Infinity : output.width);
    // The glow follows the edits, so a warmer look warms the room around the photo too
    if (ambientRef.current) drawEdited(ambientRef.current, source, edits, false, AMBIENT_WIDTH);
  }, [source, edits, cropping, output.width]);

  const shown = shownSize(pixels, edits, cropping);
  const room = { width: area.width - MARGIN * 2, height: area.height - MARGIN * 2 - (resizing ? SIZE_LABEL_ROOM * 2 : 0) };
  const fit = Math.min(1, room.width / shown.width, room.height / shown.height);
  const full = { width: Math.max(1, Math.floor(shown.width * fit)), height: Math.max(1, Math.floor(shown.height * fit)) };
  // While resizing, the photo shrinks inside the outline of its size before resizing
  const scale = resizing ? output.width / unresized.width : 1;
  const shownScale = Math.max(scale, MIN_SHOWN_SCALE);
  const size = {
    width: Math.max(1, Math.round(full.width * shownScale)),
    height: Math.max(1, Math.round(full.height * shownScale)),
  };

  return (
    <div className="ed-stage" ref={areaRef}>
      <canvas ref={ambientRef} className="ed-ambient" aria-hidden="true" />
      <div className={cls("ed-outline", resizing && "resizing")} style={full}>
        {resizing && <span className="ed-outline-label">Before · {dims(unresized)}</span>}
        <div className="ed-frame" ref={frameRef} style={size}>
          <canvas ref={canvasRef} style={size} />
          {cropping && <CropOverlay crop={edits.crop} ratio={cropRatio} frameRef={frameRef} onChange={onCrop} />}
          {comparing && <span className="ed-compare-tag">Before</span>}
          {resizing && (
            <span className={cls("ed-size-tag", scale < 1 && "smaller")}>
              {dims(output)}
              {scale < 1 && <b>{Math.round(scale * 100)}%</b>}
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
