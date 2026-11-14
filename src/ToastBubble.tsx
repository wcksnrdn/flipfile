// The notification bubble, in its own small window (src-tauri/src/toast.rs) up and to the
// right of the circle. It sizes the window to itself, points its tail at the circle,
// and goes away by itself unless the pointer rests on it.

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import type { ToastData } from "./toast";
import "./ToastBubble.css";

/** Room around the bubble for its tail and shadow */
const PAD = 16;
/** How long a bubble stays, in milliseconds */
const STAY = { error: 9000, info: 7000 };

const SHOW_IN = /Mac/i.test(navigator.userAgent) ? "Show in Finder" : /Windows/i.test(navigator.userAgent) ? "Show in Explorer" : "Show";

export default function ToastBubble() {
  const [toast, setToast] = useState<ToastData | null>(null);
  const [side, setSide] = useState("bottom-left");
  const [shown, setShown] = useState(false);
  const bubble = useRef<HTMLDivElement>(null);
  const hovering = useRef(false);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => {
    invoke<ToastData | null>("toast_current").then((t) => t && setToast(t)).catch(() => {});
    const unlisten = [
      listen<ToastData>("toast", (e) => setToast(e.payload)),
      listen<string>("toast-side", (e) => setSide(e.payload)),
    ];
    return () => unlisten.forEach((p) => p.then((fn) => fn()));
  }, []);

  const close = () => {
    window.clearTimeout(timer.current);
    setShown(false);
    // Let the bubble fade before the window goes
    window.setTimeout(() => invoke("toast_close").catch(() => {}), 160);
  };

  const arm = (tone: ToastData["tone"]) => {
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      if (!hovering.current) close();
    }, STAY[tone]);
  };

  // Each new bubble: size the window to it, then show it
  useLayoutEffect(() => {
    if (!toast || !bubble.current) return;
    setShown(false);
    const rect = bubble.current.getBoundingClientRect();
    invoke<string>("toast_fit", { width: Math.ceil(rect.width) + 2 * PAD, height: Math.ceil(rect.height) + 2 * PAD })
      .then((s) => {
        setSide(s);
        requestAnimationFrame(() => setShown(true));
        arm(toast.tone);
      })
      .catch(() => {});
    return () => window.clearTimeout(timer.current);
  }, [toast]);

  if (!toast) return null;
  const output = toast.outputs?.[0];

  return (
    <div className="toast-root" style={{ padding: PAD }}>
      <div
        ref={bubble}
        className={`toast ${toast.tone} tail-${side} ${shown ? "shown" : ""}`}
        onMouseEnter={() => (hovering.current = true)}
        onMouseLeave={() => {
          hovering.current = false;
          arm(toast.tone);
        }}
      >
        <div className="toast-head">
          <span className={`toast-dot ${toast.tone}`} />
          <span className="toast-title">{toast.title}</span>
          <button className="toast-close" onClick={close} aria-label="Close">
            ×
          </button>
        </div>

        {toast.headline && (
          <div className="toast-headline">
            <span>{toast.headline}</span>
            {toast.stat && <span className="toast-stat">{toast.stat}</span>}
          </div>
        )}

        {toast.lines && (
          <ul className="toast-lines">
            {toast.lines.map((line, i) => (
              <li key={i}>
                {line.label && <span className="toast-file">{line.label}</span>}
                <span>{line.text}</span>
              </li>
            ))}
            {toast.more ? <li className="toast-more">+{toast.more} more</li> : null}
          </ul>
        )}

        {(toast.gains?.length || toast.risks?.length) && (
          <div className="toast-columns">
            {toast.gains && toast.gains.length > 0 && (
              <div className="toast-col gains">
                <div className="toast-col-title">Gains</div>
                {toast.gains.map((g) => (
                  <div key={g} className="toast-item">
                    {g}
                  </div>
                ))}
              </div>
            )}
            {toast.risks && toast.risks.length > 0 && (
              <div className="toast-col risks">
                <div className="toast-col-title">Costs</div>
                {toast.risks.map((r) => (
                  <div key={r} className="toast-item">
                    {r}
                  </div>
                ))}
              </div>
            )}
          </div>
        )}

        {(toast.footer || output) && (
          <div className="toast-foot">
            <span>{toast.footer}</span>
            {output && (
              <button className="toast-show" onClick={() => revealItemInDir(output).catch(() => {})}>
                {SHOW_IN}
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
