import { useEffect, useEffectEvent } from "react";

type Shortcuts = {
  onUndo?: () => void; // Cmd/Ctrl+Z
  onRedo?: () => void; // Cmd/Ctrl+Shift+Z, Ctrl+Y
  onSave?: () => void; // Cmd/Ctrl+S
  onEscape: () => void;
  /** Held down: the photo without its adjustments (\) */
  onCompare?: (holding: boolean) => void;
};

/** Text and number fields keep their own undo */
const typing = (target: EventTarget | null) =>
  target instanceof HTMLInputElement && target.type !== "range" && target.type !== "button";

/** The editor's keyboard shortcuts; handlers always see the latest state */
export function useEditorShortcuts(shortcuts: Shortcuts) {
  const onKey = useEffectEvent((e: KeyboardEvent, down: boolean) => {
    const { onUndo, onRedo, onSave, onEscape, onCompare } = shortcuts;
    const mod = e.metaKey || e.ctrlKey;
    const key = e.key.toLowerCase();
    if (key === "\\" && onCompare && !mod && !typing(e.target)) {
      e.preventDefault();
      if (!e.repeat) onCompare(down);
      return;
    }
    if (!down) return;
    if (mod && key === "z" && !typing(e.target)) {
      e.preventDefault();
      (e.shiftKey ? onRedo : onUndo)?.();
    } else if (e.ctrlKey && key === "y" && !typing(e.target)) {
      e.preventDefault();
      onRedo?.();
    } else if (mod && key === "s") {
      e.preventDefault();
      onSave?.();
    } else if (key === "escape") {
      e.preventDefault();
      onEscape();
    }
  });

  useEffect(() => {
    const keydown = (e: KeyboardEvent) => onKey(e, true);
    const keyup = (e: KeyboardEvent) => onKey(e, false);
    window.addEventListener("keydown", keydown);
    window.addEventListener("keyup", keyup);
    return () => {
      window.removeEventListener("keydown", keydown);
      window.removeEventListener("keyup", keyup);
    };
  }, []);
}
