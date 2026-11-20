import { cls } from "../../shared/cls";
import type { WindowPreset } from "../../workshop/workshopWindow";

type Props = { presets: WindowPreset[]; preset: string; onPreset: (id: string) => void };

/** S / M / L: how much room the editor takes beside the circle */
export default function SizeButtons({ presets, preset, onPreset }: Props) {
  return (
    <div className="ws-size-group" title="Editor size">
      {presets.map((p) => (
        <button
          key={p.id}
          type="button"
          className={cls("ws-size-btn", preset === p.id && "active")}
          onClick={() => onPreset(p.id)}
          title={p.title.replace("Window", "Editor")}
        >
          {p.label}
        </button>
      ))}
    </div>
  );
}
