import { useState, type ReactNode } from "react";
import { cls } from "../shared/cls";
import { useWindowResize } from "../workshop/hooks/useWindowResize";
import Header from "./components/Header";
import SizeButtons from "./components/SizeButtons";
import { PhotoIcon } from "./components/icons";
import { EDITOR_SIZING, loadEditorSize, saveEditorSize } from "./editorWindow";
import { useEditorDrop } from "./hooks/useEditorDrop";
import { useEditorShortcuts } from "./hooks/useEditorShortcuts";
import { usePhoto, type PhotoRequest } from "./hooks/usePhoto";
import Workbench from "./Workbench";
import "./styles/index.css";

type Props = {
  request: PhotoRequest;
  /** On its way out, before the window shrinks (panel-out in workshop/styles/motion.css) */
  leaving: boolean;
  /** Another photo was dropped on the editor */
  onOpen: (path: string) => void;
  onWindowResized: () => void;
  onSaved: (output: string) => void;
  onClose: () => void;
};

/**
 * The Photo Editor, a card beside the circle like the Ring Workshop: a photo dropped on
 * the ring's Edit slice (or on the editor itself) opens here with its tools under it.
 */
export default function EditorPanel({ request, leaving, onOpen, onWindowResized, onSaved, onClose }: Props) {
  const photo = usePhoto(request);
  const [initialSize] = useState(loadEditorSize);
  const dropping = useEditorDrop(onOpen);
  const resize = useWindowResize(onWindowResized, {
    sizing: EDITOR_SIZING,
    initialPreset: initialSize.preset,
    onSized: saveEditorSize,
  });
  const sizes = <SizeButtons presets={resize.presets} preset={resize.preset} onPreset={resize.applyPreset} />;

  return (
    <section className={cls("workshop glass ed", leaving && "leaving")} aria-label="Photo Editor">
      {photo.preview ? (
        // A new photo starts with fresh edits and history
        <Workbench
          key={request.id}
          name={photo.name}
          preview={photo.preview}
          dropping={dropping}
          sizes={sizes}
          onSaved={onSaved}
          onClose={onClose}
        />
      ) : (
        <Placeholder name={photo.name} error={photo.error} dropping={dropping} sizes={sizes} onClose={onClose} />
      )}

      <div className="ws-resize-grip" onPointerDown={resize.onGripDown} title="Drag corner to resize the editor">
        <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true">
          <path d="M12 3L3 12M12 7L7 12M12 11L11 12" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
        </svg>
      </div>
    </section>
  );
}

type PlaceholderProps = {
  name: string;
  error?: string;
  dropping: boolean;
  sizes: ReactNode;
  onClose: () => void;
};

/** While the photo opens, or when it could not be */
function Placeholder({ name, error, dropping, sizes, onClose }: PlaceholderProps) {
  useEditorShortcuts({ onEscape: onClose });
  return (
    <>
      <Header name={name}>
        {sizes}
        <button type="button" className="ws-btn ws-btn-ghost" onClick={onClose} title="Close (Esc)">
          Done
        </button>
      </Header>
      <div className={cls("ed-photo ed-empty", dropping && "dropping")}>
        {error ? (
          <>
            <PhotoIcon />
            <span className="ed-empty-title">This photo could not be opened</span>
            <span className="ed-empty-hint">{error}</span>
            <span className="ed-empty-hint">Drop another photo here to edit it.</span>
          </>
        ) : (
          <>
            <span className="ed-spinner" aria-hidden="true" />
            <span className="ed-empty-hint">Opening {name}…</span>
          </>
        )}
      </div>
    </>
  );
}
