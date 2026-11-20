import { useReducer, useState, type ReactNode } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { cls } from "../shared/cls";
import Header from "./components/Header";
import PhotoCanvas from "./components/PhotoCanvas";
import ToolTabs, { type Tool } from "./components/ToolTabs";
import { CompareIcon, RedoIcon, UndoIcon } from "./components/icons";
import AdjustPanel from "./components/panels/AdjustPanel";
import CropPanel, { type Aspect } from "./components/panels/CropPanel";
import LooksPanel from "./components/panels/LooksPanel";
import ResizePanel from "./components/panels/ResizePanel";
import { useEditorShortcuts } from "./hooks/useEditorShortcuts";
import { photoName } from "./hooks/usePhoto";
import { saveEdits, type Preview } from "./lib/backend";
import { freshHistory, historyReducer } from "./lib/history";
import {
  FULL_CROP,
  NO_EDITS,
  fitAspect,
  flip,
  outputSize,
  rotatedSize,
  sameEdits,
  turn,
  type Crop,
  type Edits,
} from "./lib/pipeline";

type SaveState =
  | { status: "idle" }
  | { status: "saving" }
  | { status: "saved"; output: string }
  | { status: "failed"; error: string };

type Props = {
  name: string;
  preview: Preview;
  /** A file is being dragged over the editor */
  dropping: boolean;
  /** S / M / L */
  sizes: ReactNode;
  onSaved: (output: string) => void;
  onClose: () => void;
};

const sameCrop = (a: Crop, b: Crop) => a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;

/** Editing one photo: the photo, its tools under it, and saving it */
export default function Workbench({ name, preview, dropping, sizes, onSaved, onClose }: Props) {
  const [history, dispatch] = useReducer(historyReducer, undefined, freshHistory);
  const [tool, setTool] = useState<Tool>("adjust");
  const [aspect, setAspect] = useState<Aspect>(null);
  const [comparing, setComparing] = useState(false);
  const [saved, setSaved] = useState<Edits>(NO_EDITS);
  const [save, setSave] = useState<SaveState>({ status: "idle" });
  const [confirmingClose, setConfirmingClose] = useState(false);

  const edits = history.present;
  const { original } = preview;
  const frame = rotatedSize(original, edits.rotation);
  const output = outputSize(original, edits);
  const unsaved = !sameEdits(edits, saved);
  const edited = !sameEdits(edits, NO_EDITS);
  const saving = save.status === "saving";
  const canUndo = history.past.length > 0 || !sameEdits(history.present, history.committed);

  // The crop's locked shape, in pixels and in fractions of the frame
  const ratio = aspect === "original" ? frame.width / frame.height : aspect;
  const cropRatio = ratio === null ? null : (ratio * frame.height) / frame.width;

  const set = (next: Edits) => dispatch({ type: "set", edits: next, commit: true });
  const patch = (changes: Partial<Edits>, commit: boolean) => dispatch({ type: "patch", patch: changes, commit });
  const commit = () => dispatch({ type: "commit" });

  const chooseAspect = (next: Aspect) => {
    setAspect(next);
    if (next === null) return; // free: the crop stays as it is, now resizable any way
    const crop = fitAspect(next === "original" ? null : next, frame);
    patch({ crop: sameCrop(crop, FULL_CROP) ? null : crop }, true);
  };

  const turnPhoto = (direction: "cw" | "ccw") => {
    set(turn(edits, direction));
    // A locked shape turns with the photo (16:9 becomes 9:16)
    setAspect((a) => (typeof a === "number" ? 1 / a : a));
  };

  const savePhoto = async () => {
    if (saving || !edited) return false;
    const snapshot = edits;
    setSave({ status: "saving" });
    try {
      const output = await saveEdits(snapshot);
      setSaved(snapshot);
      setSave({ status: "saved", output });
      onSaved(output);
      return true;
    } catch (e) {
      setSave({ status: "failed", error: String(e) });
      return false;
    }
  };

  // Something to save: the main button saves it and closes; otherwise it just closes
  const toSave = edited && unsaved;
  // Cancel never loses edits without asking
  const cancel = () => (toSave ? setConfirmingClose(true) : onClose());
  const finish = async () => {
    if (!toSave) onClose();
    else if (await savePhoto()) onClose();
  };

  useEditorShortcuts({
    onUndo: () => dispatch({ type: "undo" }),
    onRedo: () => dispatch({ type: "redo" }),
    // Saves and keeps editing (the main button saves and closes)
    onSave: () => void savePhoto(),
    onEscape: () => (confirmingClose ? setConfirmingClose(false) : cancel()),
    onCompare: setComparing,
  });

  const resized = output.width !== original.width || output.height !== original.height;
  const detail = (
    <>
      {original.width} × {original.height}
      {resized && (
        <>
          {" → "}
          <b>
            {output.width} × {output.height}
          </b>
        </>
      )}
      {save.status === "saved" && (
        <button
          type="button"
          className="ed-saved"
          onClick={() => revealItemInDir(save.output).catch(() => {})}
          title="Show in the file manager"
        >
          ✓ Saved as {photoName(save.output)}
        </button>
      )}
    </>
  );

  return (
    <>
      <Header name={name} detail={detail} pixels={preview.pixels}>
        {sizes}
        <div className="ed-icon-group">
          <button
            type="button"
            className="ed-icon"
            disabled={!canUndo}
            onClick={() => dispatch({ type: "undo" })}
            title="Undo (Cmd/Ctrl+Z)"
          >
            <UndoIcon />
          </button>
          <button
            type="button"
            className="ed-icon"
            disabled={history.future.length === 0}
            onClick={() => dispatch({ type: "redo" })}
            title="Redo (Shift+Cmd/Ctrl+Z)"
          >
            <RedoIcon />
          </button>
        </div>
        <button type="button" className="ws-btn ws-btn-ghost ed-cancel" onClick={cancel} title="Close without saving (Esc)">
          Cancel
        </button>
        <button
          type="button"
          className={cls("ws-btn ws-btn-primary ed-finish", saving && "saving")}
          disabled={saving}
          onClick={() => void finish()}
          title={toSave ? "Save a copy next to the original and close" : "Close the editor"}
        >
          {saving ? "Saving…" : toSave ? "Save" : "Done"}
        </button>
      </Header>

      {confirmingClose && (
        <div className="ed-banner" role="alertdialog" aria-label="Discard edits">
          <span>Discard your edits? The original photo is never changed.</span>
          <button type="button" className="ws-btn ws-btn-sm ws-btn-ghost" onClick={() => setConfirmingClose(false)}>
            Keep Editing
          </button>
          <button type="button" className="ws-btn ws-btn-sm ws-btn-ghost ed-danger" onClick={onClose}>
            Discard
          </button>
        </div>
      )}
      {save.status === "failed" && (
        <div className="ed-banner error" role="alert">
          <span>{save.error}</span>
          <button type="button" className="ws-btn ws-btn-sm ws-btn-ghost" onClick={() => setSave({ status: "idle" })}>
            OK
          </button>
        </div>
      )}

      <div className="ed-photo">
        <PhotoCanvas
          pixels={preview.pixels}
          original={original}
          edits={edits}
          comparing={comparing}
          cropping={tool === "crop"}
          resizing={tool === "resize"}
          cropRatio={cropRatio}
          onCrop={(crop, done) => patch({ crop }, done)}
        />
        {edited && tool !== "crop" && (
          <button
            type="button"
            className="ed-compare"
            onPointerDown={() => setComparing(true)}
            onPointerUp={() => setComparing(false)}
            onPointerLeave={() => setComparing(false)}
            title="Hold to see the photo before its adjustments (\)"
          >
            <CompareIcon />
            Hold to compare
          </button>
        )}
      </div>

      <div className="ed-dock">
        <div className="ed-dock-bar">
          <ToolTabs tool={tool} onTool={setTool} />
          <button type="button" className="ed-revert" disabled={!edited} onClick={() => set(NO_EDITS)} title="Undo every edit">
            Revert to Original
          </button>
        </div>
        <div className="ed-controls" key={tool}>
          {tool === "adjust" && (
            <AdjustPanel adjust={edits.adjust} onChange={(adjust, done) => patch({ adjust }, done)} onCommit={commit} />
          )}
          {tool === "looks" && <LooksPanel pixels={preview.pixels} edits={edits} onPick={(adjust) => patch({ adjust }, true)} />}
          {tool === "crop" && (
            <CropPanel
              aspect={aspect}
              cropped={edits.crop !== null}
              onAspect={chooseAspect}
              onTurn={turnPhoto}
              onFlip={(axis) => set(flip(edits, axis))}
              onResetCrop={() => {
                setAspect(null);
                patch({ crop: null }, true);
              }}
            />
          )}
          {tool === "resize" && <ResizePanel original={original} edits={edits} onScale={(scale) => patch({ scale }, true)} />}
        </div>
      </div>

      {dropping && (
        <div className="ed-drop">
          <span className="ed-drop-title">Drop to edit this photo</span>
          {unsaved && edited && <span className="ed-drop-hint">Your unsaved edits will be lost</span>}
        </div>
      )}
    </>
  );
}
