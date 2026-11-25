import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTauriEvent } from "../shared/tauri";
import { setPacks, type PackProgress, type PackStatus } from "./packs";

const errorText = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : "Something went wrong");

/**
 * The AI feature packs: where each is, a download's progress, and what went wrong, kept in
 * step with the backend's `packs`, `pack-progress` and `pack-failed` events
 */
export function usePacks() {
  const [packs, setList] = useState<PackStatus[]>([]);
  const [progress, setProgress] = useState<Record<string, PackProgress>>({});
  const [failed, setFailed] = useState<Record<string, string>>({});

  const known = useCallback((list: PackStatus[]) => {
    setPacks(list);
    setList(list);
  }, []);

  /** `refresh` also looks for newer versions (the backend does at most every few hours) */
  const load = useCallback(
    (refresh: boolean) => invoke<PackStatus[]>("packs_status", { refresh }).then(known).catch(() => {}),
    [known]
  );
  useEffect(() => {
    load(false).then(() => load(true));
  }, [load]);

  useTauriEvent<PackStatus[]>("packs", known);
  useTauriEvent<PackProgress>("pack-progress", (p) => setProgress((prev) => ({ ...prev, [p.id]: p })));
  useTauriEvent<{ id: string; message: string }>("pack-failed", ({ id, message }) =>
    setFailed((prev) => ({ ...prev, [id]: message }))
  );

  const clearFailure = (id: string) =>
    setFailed((prev) => {
      const { [id]: _, ...rest } = prev;
      return rest;
    });

  return {
    packs,
    progress,
    failed,
    load,
    install: (id: string) => {
      clearFailure(id);
      setProgress((prev) => ({ ...prev, [id]: { id, done: 0, total: 0 } }));
      invoke("pack_install", { id }).catch((e) => setFailed((prev) => ({ ...prev, [id]: errorText(e) })));
    },
    cancel: (id: string) => invoke("pack_cancel", { id }).catch(() => {}),
    remove: (id: string) => {
      clearFailure(id);
      invoke<PackStatus[]>("pack_remove", { id })
        .then(known)
        .catch((e) => setFailed((prev) => ({ ...prev, [id]: errorText(e) })));
    },
  };
}

export type Packs = ReturnType<typeof usePacks>;
