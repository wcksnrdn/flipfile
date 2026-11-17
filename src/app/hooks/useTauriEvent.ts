import { useEffect, useEffectEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import { subscription } from "../lib/utils";

/**
 * Listens to a backend event for the component's lifetime. The handler always sees the
 * latest props and state, so it never acts on a stale render.
 */
export function useTauriEvent<T>(event: string, handler: (payload: T) => void) {
  const onEvent = useEffectEvent(handler);
  useEffect(() => subscription(listen<T>(event, ({ payload }) => onEvent(payload))), [event]);
}
