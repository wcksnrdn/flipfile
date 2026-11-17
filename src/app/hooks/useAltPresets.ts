import { useEffect, useEffectEvent, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { RingId } from "../../rings/dropTarget";
import { ALT_POLL_MS, ALT_SUPPORTED } from "../lib/constants";
import type { RingNavigation } from "./useRingNavigation";
import type { RingSelection } from "./useRingSelection";

type Options = {
  /** Files are in hand and the ring is showing (not the Workshop) */
  enabled: boolean;
  nav: RingNavigation;
  selection: RingSelection;
};

/**
 * Holding Option/Alt while the ring is open swaps in the size/quality presets.
 * Polled, because OS drag events carry no modifier keys (see keys.rs).
 */
export function useAltPresets({ enabled, nav, selection }: Options) {
  const ringBeforeAlt = useRef<RingId>(nav.getEntryRing());

  const poll = useEffectEvent(async () => {
    const down = await invoke<boolean>("option_key_down").catch(() => false);
    if (down === selection.altHeld.current) return;
    selection.altHeld.current = down;
    if (down) {
      const current = nav.getCurrentRing();
      ringBeforeAlt.current = current === "size" ? nav.getEntryRing() : current;
    }
    nav.switchRing(down ? "size" : ringBeforeAlt.current);
    // Re-evaluate where the cursor already is, so nothing waits for the next mouse move
    selection.reaim();
  });

  useEffect(() => {
    if (!enabled || !ALT_SUPPORTED) return;
    const timer = window.setInterval(() => poll(), ALT_POLL_MS);
    return () => window.clearInterval(timer);
  }, [enabled]);
}
