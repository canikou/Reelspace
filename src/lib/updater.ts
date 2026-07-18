import { isTauri } from "@tauri-apps/api/core";
import type { Update } from "@tauri-apps/plugin-updater";

export type UpdatePhase = "idle" | "checking" | "available" | "downloading" | "downloaded" | "installing" | "error";

export interface UpdateViewState {
  phase: UpdatePhase;
  version?: string;
  notes?: string;
  downloadedBytes?: number;
  totalBytes?: number;
  error?: string;
}

let pendingUpdate: Update | null = null;
let checkedThisSession = false;

export async function checkForUpdate({ manual = false, includePrerelease = true }: { manual?: boolean; includePrerelease?: boolean } = {}): Promise<UpdateViewState> {
  if (!isTauri()) return { phase: "idle" };
  if (!manual && checkedThisSession) return { phase: "idle" };
  checkedThisSession = true;
  const { check } = await import("@tauri-apps/plugin-updater");
  const update = await check();
  if (!update) return { phase: "idle" };
  if (!includePrerelease && update.version.includes("-")) {
    await update.close();
    return { phase: "idle" };
  }
  if (localStorage.getItem("reelspace.update.skip") === update.version) {
    await update.close();
    return { phase: "idle" };
  }
  pendingUpdate = update;
  return { phase: "available", version: update.version, notes: update.body ?? undefined };
}

export async function downloadUpdate(onProgress: (state: UpdateViewState) => void): Promise<UpdateViewState> {
  if (!pendingUpdate) throw new Error("No update is ready to download");
  let downloadedBytes = 0;
  let totalBytes: number | undefined;
  onProgress({ phase: "downloading", version: pendingUpdate.version, downloadedBytes, totalBytes });
  await pendingUpdate.download((event) => {
    if (event.event === "Started") totalBytes = event.data.contentLength ?? undefined;
    if (event.event === "Progress") downloadedBytes += event.data.chunkLength;
    onProgress({ phase: "downloading", version: pendingUpdate?.version, downloadedBytes, totalBytes });
  });
  return { phase: "downloaded", version: pendingUpdate.version, downloadedBytes, totalBytes };
}

export async function installUpdate(): Promise<void> {
  if (!pendingUpdate) throw new Error("No downloaded update is ready to install");
  await pendingUpdate.install();
  const { relaunch } = await import("@tauri-apps/plugin-process");
  await relaunch();
}

export async function dismissUpdate(version?: string, skip = false): Promise<void> {
  if (skip && version) localStorage.setItem("reelspace.update.skip", version);
  if (pendingUpdate) await pendingUpdate.close();
  pendingUpdate = null;
}
