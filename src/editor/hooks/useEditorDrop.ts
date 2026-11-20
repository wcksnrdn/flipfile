import { useEffect, useEffectEvent, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { subscription } from "../../shared/tauri";

/** Another photo dropped on the editor opens in it; returns whether one is over it */
export function useEditorDrop(onDrop: (path: string) => void) {
  const [over, setOver] = useState(false);
  const drop = useEffectEvent(onDrop);
  useEffect(
    () =>
      subscription(
        getCurrentWebview().onDragDropEvent(({ payload }) => {
          if (payload.type === "enter" || payload.type === "over") setOver(true);
          else if (payload.type === "leave") setOver(false);
          else if (payload.type === "drop") {
            setOver(false);
            const [path] = payload.paths;
            if (path) drop(path);
          }
        })
      ),
    []
  );
  return over;
}
