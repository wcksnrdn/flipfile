import type { RingId } from "../../rings/dropTarget";
import { cls } from "../../shared/cls";
import { EDITABLE_RINGS, type WorkspaceConfig } from "../workshop";

type Mode = WorkspaceConfig["mode"];

const MODES: { id: Mode; title: string; desc: string }[] = [
  { id: "direct", title: "Direct Ring (1-Step)", desc: "Open your favorite formats on a single ring right away" },
  { id: "rooms", title: "Category Rooms", desc: "Start with Photo, Office, Video, and Music rooms" },
];

type Props = {
  mode: Mode;
  editingRing: RingId;
  onSetMode: (mode: Mode) => void;
  onSelectEditingRing: (ring: RingId) => void;
};

/** Direct 1-ring vs the four category rooms, and which room ring is being edited */
export default function ModeBar({ mode, editingRing, onSetMode, onSelectEditingRing }: Props) {
  return (
    <div className="ws-mode-bar">
      <div className="ws-mode-cards" role="tablist">
        {MODES.map((m) => (
          <button
            key={m.id}
            type="button"
            className={cls("ws-mode-card", mode === m.id && "active")}
            onClick={() => onSetMode(m.id)}
          >
            <span className="ws-mode-card-title">{m.title}</span>
            <span className="ws-mode-card-desc">{m.desc}</span>
          </button>
        ))}
      </div>

      {mode === "rooms" && (
        <div className="ws-ring-tabs-wrap">
          <span className="ws-ring-tabs-label">Editing Ring:</span>
          <div className="ws-ring-tabs">
            {EDITABLE_RINGS.map((r) => (
              <button
                key={r.id}
                type="button"
                className={cls("ws-ring-tab", editingRing === r.id && "active")}
                onClick={() => onSelectEditingRing(r.id)}
              >
                {r.label}
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
