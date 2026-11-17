import { useEffect, useEffectEvent } from "react";

type Shortcuts = {
  onQuit: () => void; // Cmd/Ctrl+Q
  onToggleWorkshop: () => void; // Cmd/Ctrl+,
  onPaste: () => void; // Cmd/Ctrl+V, outside text fields
  onEscape: () => void;
};

/** The app's global keyboard shortcuts; handlers always see the latest state */
export function useKeyboardShortcuts(shortcuts: Shortcuts) {
  const onKey = useEffectEvent((e: KeyboardEvent) => {
    const mod = e.metaKey || e.ctrlKey;
    const key = e.key.toLowerCase();
    if (mod && key === "q") {
      e.preventDefault();
      shortcuts.onQuit();
    } else if (mod && key === ",") {
      e.preventDefault();
      shortcuts.onToggleWorkshop();
    } else if (mod && key === "v") {
      if ((e.target as HTMLElement)?.tagName === "INPUT") return;
      e.preventDefault();
      shortcuts.onPaste();
    } else if (key === "escape") {
      shortcuts.onEscape();
    }
  });

  useEffect(() => {
    const listener = (e: KeyboardEvent) => onKey(e);
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, []);
}
