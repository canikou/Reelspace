import { Download, FolderPlus, Save, Upload } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { normalizeAppError } from "../lib/api";
import { pickFolder, pickLibraryDataExport, pickLibraryDataImport } from "../lib/dialog";
import { formatBytes } from "../lib/format";
import type { AppSettings, AppSnapshot, LibraryDataExportSummary, LibraryDataImportSummary } from "../lib/types";

export function SettingsPage({
  snapshot,
  onSave,
  onExportData,
  onImportData
}: {
  snapshot: AppSnapshot;
  onSave: (settings: AppSettings) => Promise<void>;
  onExportData: (path: string) => Promise<LibraryDataExportSummary>;
  onImportData: (path: string) => Promise<LibraryDataImportSummary>;
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
  const [dataBusy, setDataBusy] = useState<"export" | "import" | null>(null);
  const [dataStatus, setDataStatus] = useState<string | null>(null);
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

  async function exportData() {
    setError(null);
    setDataStatus(null);
    try {
      const path = await pickLibraryDataExport();
      if (!path) return;
      setDataBusy("export");
      const summary = await onExportData(path);
      setDataStatus(`Exported ${summary.items} media reference${summary.items === 1 ? "" : "s"} and ${summary.collections} collection${summary.collections === 1 ? "" : "s"} (${formatBytes(summary.bytes)}).`);
    } catch (reason) {
      setError(normalizeAppError(reason).message);
    } finally {
      setDataBusy(null);
    }
  }

  async function importData() {
    setError(null);
    setDataStatus(null);
    try {
      const path = await pickLibraryDataImport();
      if (!path) return;
      if (!window.confirm("Import this Reelspace data archive? Tags and collection membership will be merged. Existing notes are preserved when they conflict.")) return;
      setDataBusy("import");
      const summary = await onImportData(path);
      const unresolved = summary.unmatchedItems ? ` ${summary.unmatchedItems} reference${summary.unmatchedItems === 1 ? " was" : "s were"} not matched to a local file.` : "";
      const tagConflicts = summary.tagConflicts ? ` ${summary.tagConflicts} item${summary.tagConflicts === 1 ? " exceeded" : "s exceeded"} the tag limit; existing tags were prioritized.` : "";
      const conflicts = summary.noteConflicts ? ` ${summary.noteConflicts} existing note conflict${summary.noteConflicts === 1 ? " was" : "s were"} preserved.` : "";
      setDataStatus(`Imported metadata for ${summary.updatedItems} item${summary.updatedItems === 1 ? "" : "s"}; matched ${summary.matchedItems} and restored ${summary.restoredItems}.${unresolved}${tagConflicts}${conflicts}`);
    } catch (reason) {
      setError(normalizeAppError(reason).message);
    } finally {
      setDataBusy(null);
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
      <section className="settings-panel">
        <h2>Library data</h2>
        <p className="muted">Move tags, notes, collections, and clip references without copying video, thumbnails, or previews.</p>
        <button className="secondary-button" onClick={exportData} disabled={dataBusy !== null} title="Export portable library data">
          <Download size={17} />
          {dataBusy === "export" ? "Exporting..." : "Export Library Data"}
        </button>
        <button className="secondary-button" onClick={importData} disabled={dataBusy !== null} title="Import portable library data">
          <Upload size={17} />
          {dataBusy === "import" ? "Importing..." : "Import Library Data"}
        </button>
        {dataStatus ? <p className="data-status">{dataStatus}</p> : null}
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
