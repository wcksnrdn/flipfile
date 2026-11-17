import { useState } from "react";
import { cls } from "../../shared/cls";
import type { WorkspacesState } from "../workshop";

const MAX_NAME_LENGTH = 22;

type Props = {
  state: WorkspacesState;
  activeId: string;
  onSelect: (id: string) => void;
  onCreate: (name: string) => void;
  onDelete: (id: string) => void;
};

/** Step 1: the workspace presets, and naming a new one */
export default function WorkspacePicker({ state, activeId, onSelect, onCreate, onDelete }: Props) {
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");

  const submit = () => {
    const trimmed = name.trim();
    if (!trimmed) return;
    onCreate(trimmed);
    setName("");
    setCreating(false);
  };

  return (
    <div className="ws-workspaces">
      <div className="ws-row-header">
        <span className="ws-section-label">1. Workspace Preset</span>
        <span className="ws-helper-text">Switch profiles for different workflows</span>
      </div>
      <div className="ws-pill-row">
        {state.workspaces.map((ws) => (
          <div key={ws.id} className={cls("ws-preset-pill", ws.id === activeId && "active")}>
            <button type="button" className="ws-preset-select" onClick={() => onSelect(ws.id)}>
              {ws.name}
            </button>
            {!ws.builtin && state.workspaces.length > 1 && (
              <button type="button" className="ws-preset-del" onClick={() => onDelete(ws.id)} title="Delete workspace">
                ×
              </button>
            )}
          </div>
        ))}
        {creating ? (
          <div className="ws-new-form">
            <input
              type="text"
              className="ws-new-input"
              placeholder="Workspace name..."
              value={name}
              autoFocus
              maxLength={MAX_NAME_LENGTH}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
                if (e.key === "Escape") setCreating(false);
              }}
            />
            <button type="button" className="ws-btn ws-btn-primary ws-btn-sm" onClick={submit}>
              Save
            </button>
            <button type="button" className="ws-btn ws-btn-ghost ws-btn-sm" onClick={() => setCreating(false)}>
              Cancel
            </button>
          </div>
        ) : (
          <button
            type="button"
            className="ws-add-ws-btn"
            onClick={() => setCreating(true)}
            title="Create a new custom workspace"
          >
            + New Workspace
          </button>
        )}
      </div>
    </div>
  );
}
