import type { Adjust } from "../../lib/pipeline";
import Slider from "../Slider";

type Props = {
  adjust: Adjust;
  onChange: (adjust: Adjust, commit: boolean) => void;
  onCommit: () => void;
};

const SLIDERS: { key: keyof Adjust; label: string }[] = [
  { key: "brightness", label: "Brightness" },
  { key: "contrast", label: "Contrast" },
  { key: "saturation", label: "Saturation" },
  { key: "warmth", label: "Warmth" },
];

/** Light and colour, two sliders a row */
export default function AdjustPanel({ adjust, onChange, onCommit }: Props) {
  return (
    <div className="ed-adjust">
      {SLIDERS.map(({ key, label }) => (
        <Slider
          key={key}
          label={label}
          kind={key}
          value={adjust[key]}
          onInput={(v) => onChange({ ...adjust, [key]: v }, false)}
          onCommit={onCommit}
        />
      ))}
    </div>
  );
}
