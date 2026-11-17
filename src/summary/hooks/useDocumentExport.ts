import { invoke } from "@tauri-apps/api/core";
import { useTransient } from "../../shared/useTransient";

export type Flash = "copied" | "saved";

// How long the copy/save buttons show their tick
const FLASH_MS = 1600;

type Options = {
  file: number;
  finished: boolean;
  onSaved: (file: number, path: string) => void;
  onError: (message: string) => void;
};

/** Copying the finished summary as text, and saving it next to the document */
export function useDocumentExport({ file, finished, onSaved, onError }: Options) {
  const [flash, showFlash] = useTransient<Flash>(FLASH_MS);

  const copy = () => {
    if (!finished) return;
    invoke("summary_copy", { file })
      .then(() => showFlash("copied"))
      .catch((e) => onError(String(e)));
  };

  const save = () => {
    if (!finished) return;
    invoke<string>("summary_save", { file })
      .then((path) => {
        onSaved(file, path);
        showFlash("saved");
      })
      .catch((e) => onError(String(e)));
  };

  return { flash, copy, save };
}
