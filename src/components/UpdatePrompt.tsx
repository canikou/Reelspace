import { formatBytes } from "../lib/format";
import type { UpdateViewState } from "../lib/updater";
import { AppIcon } from "./icons";

export function UpdatePrompt({ state, activeWork, onDownload, onInstall, onLater, onSkip }: {
  state: UpdateViewState;
  activeWork: boolean;
  onDownload: () => void;
  onInstall: () => void;
  onLater: () => void;
  onSkip: () => void;
}) {
  if (!state.version || state.phase === "idle" || state.phase === "checking") return null;
  const progress = state.totalBytes ? Math.min(100, (state.downloadedBytes ?? 0) / state.totalBytes * 100) : 0;
  return (
    <aside className="update-prompt" role="status" aria-live="polite">
      <div className="update-glyph"><AppIcon name={state.phase === "error" ? "error" : "update"} size={21} /></div>
      <div className="update-copy">
        <strong>{state.phase === "downloaded" ? "Update ready" : `Reelspace ${state.version} is available`}</strong>
        <p>{state.error ?? (activeWork ? "Installation will wait until active media work is finished." : state.notes || "A signed update is ready from GitHub Releases.")}</p>
        {state.phase === "downloading" ? (
          <div className="update-progress"><span style={{ width: `${progress}%` }} /><small>{state.totalBytes ? `${formatBytes(state.downloadedBytes ?? 0)} / ${formatBytes(state.totalBytes)}` : "Downloading signed update"}</small></div>
        ) : null}
      </div>
      <div className="update-actions">
        {state.phase === "available" || state.phase === "error" ? <button className="primary-button" onClick={onDownload}><AppIcon name="download" size={16} /> Download</button> : null}
        {state.phase === "downloaded" ? <button className="primary-button" onClick={onInstall} disabled={activeWork}><AppIcon name="update" size={16} /> Restart and install</button> : null}
        <button className="secondary-button" onClick={onLater}>Remind me later</button>
        <button className="text-button" onClick={onSkip}>Skip this version</button>
      </div>
    </aside>
  );
}
