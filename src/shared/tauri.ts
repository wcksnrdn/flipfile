import { useEffect, useEffectEvent } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/**
 * Ties a Tauri subscription to an effect: returns the effect's cleanup, which also
 * unsubscribes when the component unmounts before the subscription has resolved.
 */
export function subscription(pending: Promise<UnlistenFn>): () => void {
  let cancelled = false;
  let unlisten: UnlistenFn | undefined;
  pending.then((fn) => (cancelled ? fn() : (unlisten = fn))).catch(() => {});
  return () => {
    cancelled = true;
    unlisten?.();
  };
}

/**
 * Listens to a backend event for the component's lifetime. The handler always sees the
 * latest props and state, so it never acts on a stale render.
 */
export function useTauriEvent<T>(event: string, handler: (payload: T) => void) {
  const onEvent = useEffectEvent(handler);
  useEffect(() => subscription(listen<T>(event, ({ payload }) => onEvent(payload))), [event]);
}
