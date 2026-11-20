// The editor's commands (src-tauri/src/convert/edit.rs)

import { invoke } from "@tauri-apps/api/core";
import type { Edits, Size } from "./pipeline";

export type Preview = {
  /** At most 2048 px on its longest side, upright */
  pixels: ImageData;
  /** The photo's own size, which is what gets saved */
  original: Size;
};

/** Opens a dropped or pasted photo; its preview arrives as raw bytes (see editor_open) */
export async function openPhoto(path: string, toDownloads: boolean): Promise<Preview> {
  const bytes = await invoke<ArrayBuffer>("editor_open", { path, toDownloads });
  const head = new DataView(bytes, 0, 16);
  const [width, height, originalWidth, originalHeight] = [0, 4, 8, 12].map((at) => head.getUint32(at, true));
  const pixels = new ImageData(new Uint8ClampedArray(bytes, 16, width * height * 4), width, height);
  return { pixels, original: { width: originalWidth, height: originalHeight } };
}

/** Saves the open photo with `edits` next to the original; resolves to the new file */
export const saveEdits = (edits: Edits) => invoke<string>("editor_save", { edits });

/** Lets the backend drop the full-resolution photo */
export const closePhoto = () => invoke("editor_close").catch(() => {});
