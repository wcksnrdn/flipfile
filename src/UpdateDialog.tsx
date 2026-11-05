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

  // Toast message when up-to-date or offline
  if (toastMessage && !update) {
    return (
      <div className="update-toast glass" role="status">
        <span className="update-toast-icon">✨</span>
        <span className="update-toast-text">{toastMessage}</span>
      </div>
    );
  }

  // If no update available, render nothing
  if (!update && state.status !== "checking") {
    return null;
  }

  const percent =
    downloadProgress && downloadProgress.total && downloadProgress.total > 0
      ? Math.round((downloadProgress.downloaded / downloadProgress.total) * 100)
      : null;

  return (
    <div className="update-overlay" onClick={onDismiss}>
      <div
        className="update-dialog glass"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby="update-title"
      >
        <div className="update-dialog-header">
          <div className="update-icon-badge">🚀</div>
          <div>
            <h3 id="update-title" className="update-dialog-title">
              {installing ? "Updating flipfile…" : "New Update Available!"}
            </h3>
            <span className="update-dialog-version">
              {update ? `Version ${update.version}` : "Checking for updates…"}
            </span>
          </div>
        </div>

        {update?.body && !installing && (
          <div className="update-dialog-notes">
            <p className="update-notes-label">What's new:</p>
            <div className="update-notes-body">{update.body}</div>
          </div>
        )}

        {installing && (
          <div className="update-dialog-progress-group">
            <div className="update-progress-track">
              <div
                className="update-progress-fill"
                style={{ width: percent !== null ? `${percent}%` : "100%" }}
              />
            </div>
            <span className="update-progress-label">
              {percent !== null ? `Downloading… ${percent}%` : "Installing & relaunching…"}
            </span>
          </div>
        )}

        <div className="update-dialog-actions">
          {!installing && (
            <>
              <button
                type="button"
                className="ws-btn ws-btn-primary"
                onClick={handleInstall}
              >
                Update Now
              </button>
              <button
                type="button"
                className="ws-btn ws-btn-ghost"
                onClick={onDismiss}
              >
                Later
              </button>
            </>
          )}
          {installing && (
            <span className="update-installing-hint">Please wait, restarting soon…</span>
          )}
        </div>
      </div>
    </div>
  );
}

