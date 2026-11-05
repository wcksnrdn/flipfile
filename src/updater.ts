import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "available"; update: Update; version: string; body?: string }
  | { status: "downloading"; progress: number; total?: number }
  | { status: "ready" }
  | { status: "up-to-date" }
  | { status: "error"; message: string };

export async function checkForUpdate(): Promise<Update | null> {
  try {
    const update = await check();
    return update;
  } catch (err) {
    console.warn("Failed to check for updates:", err);
    throw err;
  }
}

export async function installUpdate(
  update: Update,
  onProgress?: (downloaded: number, total?: number) => void
): Promise<void> {
  let downloaded = 0;
  let total: number | undefined;

  await update.downloadAndInstall((event) => {
    switch (event.event) {
      case "Started":
        total = event.data.contentLength;
        onProgress?.(0, total);
        break;
      case "Progress":
        downloaded += event.data.chunkLength;
        onProgress?.(downloaded, total);
        break;
      case "Finished":
        onProgress?.(downloaded, total);
        break;
    }
  });

  await relaunch();
}

