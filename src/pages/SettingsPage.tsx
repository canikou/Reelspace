import { FolderPlus, Save } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { pickFolder } from "../lib/dialog";
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
  const [error, setError] = useState<string | null>(null);
  const saveInFlight = useRef(false);

  useEffect(() => {
    setSourceFolders(snapshot.settings.sourceFolders.join("\n"));
    setWorkingFolder(snapshot.settings.workingFolder ?? "");
    setClipFolder(snapshot.settings.clipFolder ?? "");
    setLimitGb(snapshot.settings.storageLimitBytes ? String(Math.round(snapshot.settings.storageLimitBytes / 1024 / 1024 / 1024)) : "");
    setThreshold(String(snapshot.settings.cleanupThresholdPercent));
  }, [snapshot]);

  async function save() {
    if (saveInFlight.current) return;
    saveInFlight.current = true;
    setBusy(true);
    setError(null);
    try {
      await onSave({
        sourceFolders: sourceFolders.split(/\r?\n/).map((line) => line.trim()).filter(Boolean),
        workingFolder: workingFolder.trim() || null,
        clipFolder: clipFolder.trim() || null,
        storageLimitBytes: limitGb ? Number(limitGb) * 1024 * 1024 * 1024 : null,
        cleanupThresholdPercent: Math.max(50, Math.min(99, Number(threshold) || 90))
      });
    } catch (reason) {
      setError(String(reason));
    } finally {
      saveInFlight.current = false;
      setBusy(false);
    }
  }

  async function addSourceFolder() {
    const folder = await pickFolder();
    if (!folder) return;
    const existing = sourceFolders.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
    if (!existing.some((value) => value.toLowerCase() === folder.toLowerCase())) {
      setSourceFolders([...existing, folder].join("\n"));
    }
  }

  async function chooseWorkingFolder() {
    const folder = await pickFolder();
    if (folder) setWorkingFolder(folder);
  }

  async function chooseClipFolder() {
    const folder = await pickFolder();
    if (folder) setClipFolder(folder);
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
        <button className="secondary-button" onClick={addSourceFolder} title="Add a source folder">
          <FolderPlus size={17} />
          Add Source Folder
        </button>
        <label>
          Working folder
          <input value={workingFolder} onChange={(event) => setWorkingFolder(event.target.value)} placeholder="Optional" />
        </label>
        <button className="secondary-button" onClick={chooseWorkingFolder} title="Choose working folder">
          <FolderPlus size={17} />
          Choose Working Folder
        </button>
        <label>
          Clip folder
          <input value={clipFolder} onChange={(event) => setClipFolder(event.target.value)} placeholder="Defaults to working folder" />
        </label>
        <button className="secondary-button" onClick={chooseClipFolder} title="Choose clip folder">
          <FolderPlus size={17} />
          Choose Clip Folder
        </button>
      </section>
      <section className="settings-panel">
        <h2>Storage</h2>
        <label>
          Storage limit in GB
          <input type="number" min="1" value={limitGb} onChange={(event) => setLimitGb(event.target.value)} inputMode="numeric" placeholder="Optional" />
        </label>
        <label>
          Cleanup warning threshold
          <input type="number" min="50" max="99" value={threshold} onChange={(event) => setThreshold(event.target.value)} inputMode="numeric" />
        </label>
        <p className="muted">Current managed usage: {formatBytes(snapshot.stats.managedBytes)}</p>
      </section>
      <section className="settings-panel">
        <h2>Tools</h2>
        <div className={snapshot.ffmpeg.ready ? "tool-health ready" : "tool-health"}>
          <strong>{snapshot.ffmpeg.ready ? "Ready" : "Missing"}</strong>
          <span>{snapshot.ffmpeg.message}</span>
          <details>
            <summary>Tool paths</summary>
            <code>{snapshot.ffmpeg.ffmpegPath ?? "ffmpeg.exe not found"}</code>
            <code>{snapshot.ffmpeg.ffprobePath ?? "ffprobe.exe not found"}</code>
          </details>
        </div>
      </section>
      <div className="settings-actions">
        {error ? <div className="inline-error">{error}</div> : null}
        <button className="primary-button" onClick={save} disabled={busy} title="Save settings">
          <Save size={17} />
          {busy ? "Saving..." : "Save Settings"}
        </button>
      </div>
    </div>
  );
}
