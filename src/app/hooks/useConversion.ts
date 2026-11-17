import { Channel, invoke } from "@tauri-apps/api/core";
import { failureToast, type ToastData } from "../../toast/toast";
import { AI_SUMMARY } from "../../workshop/workshop";
import { batchProgressStatus, batchReport, batchStartStatus, failedBatch } from "../lib/batchReport";
import { STATUS_CLEAR_MS } from "../lib/constants";
import type { BatchResult, Execute, Progress } from "../lib/types";
import type { StatusActions } from "./useStatus";

// The notification bubble beside the circle (src-tauri/src/toast.rs)
const showBubble = (toast: ToastData) => invoke("toast_show", { toast }).catch(() => {});

/** Runs conversions and AI summaries on the backend, reporting on the circle and the bubble */
export function useConversion(status: StatusActions) {
  const runBatch = async (paths: string[], format: string, toDownloads: boolean, preset: string | null) => {
    const job = { paths, format, preset };
    status.show(batchStartStatus(job));

    const many = paths.length > 1;
    const onProgress = new Channel<Progress>();
    onProgress.onmessage = (p) => status.update((s) => batchProgressStatus(s, p, many));

    let result: BatchResult;
    try {
      result = await invoke<BatchResult>("convert_batch", { paths, format, onProgress, toDownloads, preset });
    } catch (e) {
      result = failedBatch(e);
    }

    const report = batchReport(job, result);
    status.show(report.status, report.clearAfterMs);
    if (report.toast) showBubble(report.toast);
  };

  // Summary opens the panel beside the circle; its progress arrives as summary-progress
  const summarize = (paths: string[], toDownloads: boolean) => {
    status.show({ title: "Reading", hint: "Apple Intelligence", tone: "busy", ai: true });
    invoke("summarize", { paths, toDownloads }).catch((e) => {
      status.show({ title: "Failed", hint: String(e), tone: "err" }, STATUS_CLEAR_MS.failure);
      showBubble(failureToast([String(e)], paths.length, []));
    });
  };

  const execute: Execute = (paths, format, toDownloads, preset) =>
    format === AI_SUMMARY ? summarize(paths, toDownloads) : runBatch(paths, format, toDownloads, preset);

  return execute;
}
