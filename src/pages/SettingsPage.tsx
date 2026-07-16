import { Save } from "lucide-react";
import { useState } from "react";
import { formatBytes } from "../lib/format";
import type { AppSettings, AppSnapshot } from "../lib/types";

export function SettingsPage({
  snapshot,
  onSave
}: {
  snapshot: AppSnapshot;
  onSave: (settings: AppSettings) => Promise<void>;
}) {
  const [sourceFolders, setSourceFolders] = useState(snapshot.settings.sourceFolders.join("\n"));
  const [workingFolder, setWorkingFolder] = useState(snapshot.settings.workingFolder ?? "");
  const [clipFolder, setClipFolder] = useState(snapshot.settings.clipFolder ?? "");
  const [limitGb, setLimitGb] = useState(
    snapshot.settings.storageLimitBytes ? String(Math.round(snapshot.settings.storageLimitBytes / 1024 / 1024 / 1024)) : ""
  );
  const [threshold, setThreshold] = useState(String(snapshot.settings.cleanupThresholdPercent));
  const [busy, setBusy] = useState(false);

  async function save() {
    setBusy(true);
    try {
      await onSave({
        sourceFolders: sourceFolders.split(/\r?\n/).map((line) => line.trim()).filter(Boolean),
        workingFolder: workingFolder.trim() || null,
        clipFolder: clipFolder.trim() || null,
        storageLimitBytes: limitGb ? Number(limitGb) * 1024 * 1024 * 1024 : null,
        cleanupThresholdPercent: Math.max(50, Math.min(99, Number(threshold) || 90))
      });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="settings-layout">
      <section className="settings-panel">
        <h2>Folders</h2>
        <label>
          Source folders
          <textarea
            value={sourceFolders}
            onChange={(event) => setSourceFolders(event.target.value)}
            placeholder="One folder per line"
            rows={5}
          />
        </label>
        <label>
          Working folder
          <input value={workingFolder} onChange={(event) => setWorkingFolder(event.target.value)} placeholder="Optional" />
        </label>
        <label>
          Clip folder
          <input value={clipFolder} onChange={(event) => setClipFolder(event.target.value)} placeholder="Defaults to working folder" />
        </label>
      </section>
      <section className="settings-panel">
        <h2>Storage</h2>
        <label>
          Storage limit in GB
          <input value={limitGb} onChange={(event) => setLimitGb(event.target.value)} inputMode="numeric" placeholder="Optional" />
        </label>
        <label>
          Cleanup warning threshold
          <input value={threshold} onChange={(event) => setThreshold(event.target.value)} inputMode="numeric" />
        </label>
        <p className="muted">Current managed usage: {formatBytes(snapshot.stats.managedBytes)}</p>
      </section>
      <section className="settings-panel">
        <h2>Tools</h2>
        <div className={snapshot.ffmpeg.ready ? "tool-health ready" : "tool-health"}>
          <strong>{snapshot.ffmpeg.ready ? "Ready" : "Missing"}</strong>
          <span>{snapshot.ffmpeg.message}</span>
          <code>{snapshot.ffmpeg.ffmpegPath ?? "ffmpeg.exe not found"}</code>
          <code>{snapshot.ffmpeg.ffprobePath ?? "ffprobe.exe not found"}</code>
        </div>
      </section>
      <div className="settings-actions">
        <button className="primary-button" onClick={save} disabled={busy} title="Save settings">
          <Save size={17} />
          {busy ? "Saving..." : "Save Settings"}
        </button>
      </div>
    </div>
  );
}
