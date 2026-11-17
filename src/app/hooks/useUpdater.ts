import { useEffect, useRef, useState, type ComponentProps } from "react";
import type { Update } from "@tauri-apps/plugin-updater";
import { checkForUpdate, type UpdateState } from "../../update/updater";
import type UpdateDialog from "../../update/UpdateDialog";
import { useTauriEvent } from "./useTauriEvent";

const TOAST_MS = 3200;
// Quiet background check, once the app has settled after launch
const STARTUP_CHECK_DELAY_MS = 4000;

/**
 * The in-app updater: "Check for Updates..." from the tray or the Workshop, a quiet check
 * after launch, and the dialog's install flow.
 */
export function useUpdater() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [state, setState] = useState<UpdateState>({ status: "idle" });
  const [toastMessage, setToastMessage] = useState<string | null>(null);
  const toastTimer = useRef<number | undefined>(undefined);

  const showToast = (msg: string) => {
    window.clearTimeout(toastTimer.current);
    setToastMessage(msg);
    toastTimer.current = window.setTimeout(() => setToastMessage(null), TOAST_MS);
  };

  const offer = (up: Update) => {
    setUpdate(up);
    setState({ status: "available", update: up, version: up.version, body: up.body });
  };

  const checkNow = async () => {
    setState({ status: "checking" });
    try {
      const up = await checkForUpdate();
      if (up) {
        offer(up);
      } else {
        setState({ status: "up-to-date" });
        showToast("You're up to date · v0.11.0");
      }
    } catch (err) {
      setState({ status: "error", message: String(err) });
      showToast("Could not check for updates");
    }
  };

  useTauriEvent("check-update", () => checkNow());

  useEffect(() => {
    let cancelled = false;
    const timer = window.setTimeout(async () => {
      try {
        const up = await checkForUpdate();
        if (!cancelled && up) offer(up);
      } catch {
        // Quiet on startup background failure
      }
    }, STARTUP_CHECK_DELAY_MS);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      window.clearTimeout(toastTimer.current);
    };
  }, []);

  const dialogProps: ComponentProps<typeof UpdateDialog> = {
    update,
    state,
    toastMessage,
    onDismiss: () => {
      setUpdate(null);
      setState({ status: "idle" });
    },
    onInstallStart: () => setState({ status: "downloading", progress: 0 }),
    onProgress: (progress, total) => setState({ status: "downloading", progress, total }),
    onError: (msg) => {
      setState({ status: "error", message: msg });
      showToast(`Update error: ${msg}`);
      setUpdate(null);
    },
  };

  return { checkNow, dialogProps };
}
