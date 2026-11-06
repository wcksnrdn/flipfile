import { useState, useEffect } from "react";
import type { Update } from "@tauri-apps/plugin-updater";
import { installUpdate, type UpdateState } from "./updater";

interface UpdateDialogProps {
  update: Update | null;
  state: UpdateState;
  toastMessage: string | null;
  onDismiss: () => void;
  onInstallStart: () => void;
  onProgress: (downloaded: number, total?: number) => void;
  onError: (msg: string) => void;
}

const UpArrow = () => (
  <svg viewBox="0 0 24 24" aria-hidden="true">
    <path d="M12 18V6.5M6.5 12 12 6.5l5.5 5.5" />
  </svg>
);

const Check = () => (
  <svg viewBox="0 0 24 24" aria-hidden="true">
    <path d="m6 12.5 4 4 8-9" />
  </svg>
);

export default function UpdateDialog({
  update,
  state,
  toastMessage,
  onDismiss,
  onInstallStart,
  onProgress,
  onError,
}: UpdateDialogProps) {
  const [downloadProgress, setDownloadProgress] = useState<{ downloaded: number; total?: number } | null>(null);
  const [installing, setInstalling] = useState(false);

  useEffect(() => {
    if (state.status === "downloading") {
      setDownloadProgress({ downloaded: state.progress, total: state.total });
    }
  }, [state]);

  const handleInstall = async () => {
    if (!update) return;
    setInstalling(true);
    onInstallStart();
    try {
      await installUpdate(update, (downloaded, total) => {
        setDownloadProgress({ downloaded, total });
        onProgress(downloaded, total);
      });
    } catch (err) {
      console.error("Installation failed:", err);
      setInstalling(false);
      onError(err instanceof Error ? err.message : String(err));
    }
  };

  // Checking, and its outcome when there is nothing to install, are a small pill under
  // the circle: nothing covers the window or darkens its transparent corners
  if (!update && state.status === "checking") {
    return (
      <div className="update-toast" role="status">
        <span className="update-spinner" aria-hidden="true" />
        <span className="update-toast-text">Checking for updates…</span>
      </div>
    );
  }
  if (toastMessage && !update) {
    const ok = /up to date/i.test(toastMessage);
    return (
      <div className="update-toast" role="status">
        <span className={`update-toast-icon ${ok ? "ok" : "err"}`} aria-hidden="true">
          {ok ? <Check /> : "!"}
        </span>
        <span className="update-toast-text">{toastMessage}</span>
      </div>
    );
  }

  if (!update) {
    return null;
  }

  const percent =
    downloadProgress && downloadProgress.total && downloadProgress.total > 0
      ? Math.round((downloadProgress.downloaded / downloadProgress.total) * 100)
      : null;

  return (
    // A clear layer: clicking beside the card dismisses it, without dimming anything
    <div className="update-layer" onClick={installing ? undefined : onDismiss}>
      <div
        className="update-card"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby="update-title"
      >
        <div className="update-card-head">
          <div className={`update-badge ${installing ? "busy" : ""}`}>
            <UpArrow />
          </div>
          <div className="update-card-titles">
            <h3 id="update-title">{installing ? "Updating flipfile" : "Update available"}</h3>
            <span>Version {update.version}</span>
          </div>
        </div>

        {update.body && !installing && <div className="update-notes">{update.body}</div>}

        {installing && (
          <div className="update-progress">
            <div className="update-progress-track">
              <div
                className={`update-progress-fill ${percent === null ? "indeterminate" : ""}`}
                style={percent !== null ? { width: `${percent}%` } : undefined}
              />
            </div>
            <span>{percent !== null ? `Downloading… ${percent}%` : "Installing and restarting…"}</span>
          </div>
        )}

        {!installing && (
          <div className="update-actions">
            <button type="button" className="update-button primary" onClick={handleInstall}>
              Update now
            </button>
            <button type="button" className="update-button" onClick={onDismiss}>
              Later
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
