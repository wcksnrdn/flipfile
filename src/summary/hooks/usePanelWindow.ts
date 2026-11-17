import { useEffect, useEffectEvent, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { subscription } from "../../shared/tauri";
import type { Doc } from "../summary";

// The panel's own window (src-tauri/src/summary/window.rs): a rounded glass card
const RADIUS = 22;

/** Native glass behind the whole card (macOS); returns whether it is in use */
export function usePanelGlass() {
  const [native, setNative] = useState(false);
  useEffect(() => {
    const apply = () =>
      invoke<boolean>("set_glass", {
        cx: window.innerWidth / 2,
        cy: window.innerHeight / 2,
        diameter: 0,
        width: window.innerWidth,
        height: window.innerHeight,
        radius: RADIUS,
      })
        .then(setNative)
        .catch(() => setNative(false));
    apply();
    window.addEventListener("resize", apply);
    return () => window.removeEventListener("resize", apply);
  }, []);
  return native;
}

/** Another document dropped on the panel itself is summarized right here; returns whether one is over it */
export function usePanelDrop(onError: (message: string) => void) {
  const [dropping, setDropping] = useState(false);
  const fail = useEffectEvent(onError);
  useEffect(
    () =>
      subscription(
        getCurrentWebview().onDragDropEvent(({ payload }) => {
          if (payload.type === "enter" || payload.type === "over") setDropping(true);
          else if (payload.type === "leave") setDropping(false);
          else if (payload.type === "drop") {
            setDropping(false);
            invoke("summarize", { paths: payload.paths, toDownloads: false }).catch((e) => fail(String(e)));
          }
        })
      ),
    []
  );
  return dropping;
}

/** The current time, ticking every second while `running` (the footer's clock) */
export function useClock(running: boolean) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [running]);
  return now;
}

/** New questions and their answers scroll into view as they are written */
export function useFollowAnswers(doc: Doc | undefined) {
  const bodyRef = useRef<HTMLDivElement>(null);
  const count = doc?.answers.length ?? 0;
  const last = doc?.answers[count - 1];
  const text = last?.text;
  const done = last?.done;
  // Follows the newest answer as it grows (its text) and when it settles (done)
  useEffect(() => {
    const body = bodyRef.current;
    if (body && count > 0) body.scrollTo({ top: body.scrollHeight, behavior: "smooth" });
  }, [count, text, done]);
  return bodyRef;
}

type Shortcuts = {
  draft: string;
  clearDraft: () => void;
  close: () => void;
  copy: () => void;
  save: () => void;
};

/** Esc / Cmd+W closes, Cmd+C copies (unless text is selected), Cmd+S saves */
export function usePanelShortcuts(shortcuts: Shortcuts) {
  const onKey = useEffectEvent((e: KeyboardEvent) => {
    const { draft, clearDraft, close, copy, save } = shortcuts;
    const mod = e.metaKey || e.ctrlKey;
    const key = e.key.toLowerCase();
    const typing = (e.target as HTMLElement)?.tagName === "INPUT";
    if (typing && key === "escape" && draft) {
      // Esc in the question box clears it first, then closes the panel
      e.preventDefault();
      clearDraft();
    } else if (key === "escape" || (mod && key === "w")) {
      e.preventDefault();
      close();
    } else if (mod && key === "c" && !typing && !window.getSelection()?.toString()) {
      e.preventDefault();
      copy();
    } else if (mod && key === "s") {
      e.preventDefault();
      save();
    }
  });

  useEffect(() => {
    const listener = (e: KeyboardEvent) => onKey(e);
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, []);
}
