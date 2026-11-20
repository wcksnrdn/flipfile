import { useEffect, useState } from "react";
import { closePhoto, openPhoto, type Preview } from "../lib/backend";

/** A photo to edit: dropped on the ring's Edit slice, pasted, or dropped on the editor */
export type PhotoRequest = { id: number; path: string; toDownloads: boolean };

type Loaded = { id: number; preview?: Preview; error?: string };

/** The file's name, from its full path */
export const photoName = (path: string) => path.split(/[\\/]/).pop() || "Photo";

/** Opens the requested photo on the backend; the latest request always wins */
export function usePhoto(request: PhotoRequest) {
  const [loaded, setLoaded] = useState<Loaded | null>(null);

  useEffect(() => {
    let current = true;
    openPhoto(request.path, request.toDownloads).then(
      (preview) => current && setLoaded({ id: request.id, preview }),
      (e) => current && setLoaded({ id: request.id, error: String(e) })
    );
    return () => {
      current = false;
    };
  }, [request]);

  // The editor closed: the backend lets go of the full-resolution photo
  useEffect(() => () => void closePhoto(), []);

  const done = loaded?.id === request.id ? loaded : null;
  return {
    name: photoName(request.path),
    loading: done === null,
    preview: done?.preview,
    error: done?.error,
  };
}
