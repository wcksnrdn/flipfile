import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { hasAiSupport, setAiSupport } from "../../workshop/workshop";
import { useTauriEvent } from "./useTauriEvent";

/** The tray's resize/quality defaults, shown under the core when they are not the defaults */
export function useSettingsSummary() {
  const [summary, setSummary] = useState<string | null>(null);
  useEffect(() => {
    invoke<string | null>("settings_summary").then(setSummary).catch(() => {});
  }, []);
  useTauriEvent<string | null>("settings-changed", setSummary);
  return summary;
}

/** Whether this computer can write AI summaries (Apple Intelligence); assumed on macOS until known */
export function useAiSupport() {
  const [supported, setSupported] = useState(hasAiSupport);
  useEffect(() => {
    const known = (value: boolean) => {
      setAiSupport(value);
      setSupported(value);
    };
    invoke<{ supported: boolean }>("summary_status")
      .then((a) => known(a.supported))
      .catch(() => known(false));
  }, []);
  return supported;
}
