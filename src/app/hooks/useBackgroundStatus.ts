import { STATUS_CLEAR_MS } from "../lib/constants";
import type { SummaryProgress } from "../lib/types";
import { plural } from "../lib/utils";
import type { StatusActions } from "./useStatus";
import { useTauriEvent } from "../../shared/tauri";

/** Status the backend reports on its own: the summary panel's progress and the tray's Undo */
export function useBackgroundStatus(status: StatusActions) {
  // The summary panel's progress, shown by the circle as well
  useTauriEvent<SummaryProgress>("summary-progress", ({ title, hint, tone, progress }) => {
    if (tone === "idle") {
      status.keep();
      status.update((s) => (s?.ai ? null : s));
      return;
    }
    const clearAfterMs =
      tone === "busy" ? undefined : tone === "ok" ? STATUS_CLEAR_MS.summaryDone : STATUS_CLEAR_MS.summaryFailed;
    status.show({ title, hint, tone, progress: progress ?? undefined, ai: true }, clearAfterMs);
  });

  // "Undo Last Conversion" from the tray menu
  useTauriEvent<{ count: number; error: string | null }>("history-undone", ({ count, error }) => {
    status.show(
      error
        ? { title: "Undo failed", hint: error, tone: "err" }
        : { title: "↩ Undone", hint: `${plural(count, "file")} moved to Trash`, tone: "ok" },
      STATUS_CLEAR_MS.failure
    );
  });
}
