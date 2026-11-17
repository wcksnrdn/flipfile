import { invoke } from "@tauri-apps/api/core";
import { STATUS_CLEAR_MS } from "../lib/constants";
import type { Pasted } from "../lib/types";
import { clipboardName } from "../lib/utils";
import type { RingSelection } from "./useRingSelection";
import type { StatusActions } from "./useStatus";

/** Cmd/Ctrl+V: files or an image from the clipboard, held until a format is clicked */
export function useClipboardPaste(selection: RingSelection, status: StatusActions) {
  return async () => {
    let pasted: Pasted;
    try {
      pasted = await invoke<Pasted>("read_clipboard", { name: clipboardName() });
    } catch (e) {
      pasted = { kind: "empty" };
      console.error("read_clipboard", e);
    }

    if (pasted.kind === "empty") {
      status.show({ title: "Nothing to paste", hint: "Copy an image or files first", tone: "err" }, STATUS_CLEAR_MS.notice);
      return;
    }

    status.show(null);
    if (pasted.kind === "files") selection.startPicking(pasted.paths, false);
    else selection.startPicking([pasted.path], true);
  };
}
