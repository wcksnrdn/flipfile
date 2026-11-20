import { cls } from "../../../shared/cls";
import { FlipHIcon, FlipVIcon, RotateCcwIcon, RotateCwIcon } from "../icons";

/** Width / height in pixels; null is free, "original" is the photo's own shape */
export type Aspect = number | "original" | null;

const ASPECTS: { label: string; value: Aspect }[] = [
  { label: "Free", value: null },
  { label: "Original", value: "original" },
  { label: "Square", value: 1 },
  { label: "4:5", value: 4 / 5 },
  { label: "3:4", value: 3 / 4 },
  { label: "2:3", value: 2 / 3 },
  { label: "9:16", value: 9 / 16 },
  { label: "4:3", value: 4 / 3 },
  { label: "3:2", value: 3 / 2 },
  { label: "16:9", value: 16 / 9 },
];

type Props = {
  aspect: Aspect;
  cropped: boolean;
  onAspect: (aspect: Aspect) => void;
  onTurn: (direction: "cw" | "ccw") => void;
  onFlip: (axis: "h" | "v") => void;
  onResetCrop: () => void;
};

/** Turn and mirror on the left, the crop's shape on the right */
export default function CropPanel({ aspect, cropped, onAspect, onTurn, onFlip, onResetCrop }: Props) {
  return (
    <div className="ed-crop-tools">
      <div className="ed-icon-group">
        <button type="button" className="ed-icon" onClick={() => onTurn("ccw")} title="Rotate Left">
          <RotateCcwIcon />
        </button>
        <button type="button" className="ed-icon" onClick={() => onTurn("cw")} title="Rotate Right">
          <RotateCwIcon />
        </button>
        <button type="button" className="ed-icon" onClick={() => onFlip("h")} title="Flip Horizontal">
          <FlipHIcon />
        </button>
        <button type="button" className="ed-icon" onClick={() => onFlip("v")} title="Flip Vertical">
          <FlipVIcon />
        </button>
      </div>
      <div className="ed-chips">
        {ASPECTS.map((a) => (
          <button
            key={a.label}
            type="button"
            className={cls("ed-chip", aspect === a.value && "active")}
            onClick={() => onAspect(a.value)}
          >
            {a.label}
          </button>
        ))}
        {cropped && (
          <button type="button" className="ed-chip ed-chip-reset" onClick={onResetCrop}>
            Reset
          </button>
        )}
      </div>
    </div>
  );
}
