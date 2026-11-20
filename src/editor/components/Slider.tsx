type Props = {
  label: string;
  /** Picks the track's colours (styles/controls.css), which show what the slider does */
  kind: string;
  value: number;
  /** Live on every step */
  onInput: (value: number) => void;
  /** Once the slider is let go: one undo step */
  onCommit: () => void;
};

/** A -100..100 slider centred on 0, on a track coloured by what it does; double-click to reset */
export default function Slider({ label, kind, value, onInput, onCommit }: Props) {
  const reset = () => {
    onInput(0);
    onCommit();
  };

  return (
    <label className={`ed-slider ${kind}`} onDoubleClick={reset} title="Double-click to reset">
      <span className="ed-slider-head">
        <span>{label}</span>
        <span className={value === 0 ? "ed-slider-value" : "ed-slider-value changed"}>{value > 0 ? `+${value}` : value}</span>
      </span>
      <input
        type="range"
        min={-100}
        max={100}
        step={1}
        value={value}
        onChange={(e) => onInput(Number(e.target.value))}
        onPointerUp={onCommit}
        onKeyUp={onCommit}
        onBlur={onCommit}
      />
    </label>
  );
}
