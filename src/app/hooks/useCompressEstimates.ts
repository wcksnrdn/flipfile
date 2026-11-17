import { useEffect, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { COMPRESS_LEVELS, type Item, type Ring } from "../../rings/dropTarget";
import { formatBytes } from "../../toast/toast";
import type { Estimate } from "../lib/types";

type Options = {
  /** Files are in hand and the ring is showing (not the Workshop) */
  enabled: boolean;
  ring: Ring | undefined;
  getPaths: () => string[];
};

/**
 * A ring holding compress levels (the Option/Alt ring, or any ring given them in the
 * Workshop, the only way on Linux) works out their sizes as soon as it opens, so each
 * level's slice and the centre show what it will make of the files before the drop.
 */
export function useCompressEstimates({ enabled, ring, getPaths }: Options) {
  const [estimates, setEstimates] = useState<Record<string, Estimate>>({});
  const token = useRef(0);
  const forPaths = useRef(""); // which files the estimates are for

  useEffect(() => {
    if (!enabled || !ring?.items.some((item) => COMPRESS_LEVELS.includes(item.id))) return;
    const paths = getPaths();
    const key = paths.join("\n");
    if (paths.length === 0 || key === forPaths.current) return;
    forPaths.current = key;
    const current = ++token.current;
    setEstimates({});
    const onEstimate = new Channel<Estimate>();
    onEstimate.onmessage = (e) => {
      if (e.token === token.current) setEstimates((prev) => ({ ...prev, [e.id]: e }));
    };
    invoke("compress_estimate", { paths, token: current, onEstimate }).catch(() => {});
  }, [enabled, ring, getPaths]);

  /** A compress level's slice shows its estimated result once known */
  const sliceSub = (item: Pick<Item, "id" | "sub">) => {
    const e = estimates[item.id];
    if (!e || e.after === null || !COMPRESS_LEVELS.includes(item.id)) return item.sub;
    return e.after >= e.before ? "compact" : `${e.approximate ? "≈" : ""}${formatBytes(e.after)}`;
  };

  return { estimates, sliceSub };
}
