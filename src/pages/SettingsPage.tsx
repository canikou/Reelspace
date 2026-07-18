import { useEffect, useRef, useState } from "react";
import { AppIcon, FrameStackMark } from "../components/icons";
import { api, normalizeAppError } from "../lib/api";
import { pickDiagnosticsExport, pickFolder, pickLibraryDataExport, pickLibraryDataImport } from "../lib/dialog";
import { formatBytes } from "../lib/format";
import type { AppSettings, AppSnapshot, ArchiveAccount, LibraryDataExportSummary, LibraryDataImportSummary, RecorderApp } from "../lib/types";

export function SettingsPage({
  snapshot,
  onSave,
  onExportData,
  onImportData,
  recorders = [],
  archiveAccount = null,
  onConnectYouTube = async () => undefined,
  onDisconnectYouTube = async () => undefined,
}: {
  snapshot: AppSnapshot;
  onSave: (settings: AppSettings) => Promise<void>;
  onExportData: (path: string) => Promise<LibraryDataExportSummary>;
  onImportData: (path: string) => Promise<LibraryDataImportSummary>;
  recorders?: RecorderApp[];
  archiveAccount?: ArchiveAccount | null;
  onConnectYouTube?: () => Promise<unknown>;
  onDisconnectYouTube?: () => Promise<void>;
}) {
  const [sourceFolders, setSourceFolders] = useState(snapshot.settings.sourceFolders.join("\n"));
  const [workingFolder, setWorkingFolder] = useState(snapshot.settings.workingFolder ?? "");
  const [clipFolder, setClipFolder] = useState(snapshot.settings.clipFolder ?? "");
  const [limitGb, setLimitGb] = useState(
    snapshot.settings.storageLimitBytes ? String(Math.round(snapshot.settings.storageLimitBytes / 1024 / 1024 / 1024)) : ""
  );
  const [threshold, setThreshold] = useState(String(snapshot.settings.cleanupThresholdPercent));
  const [theme, setTheme] = useState(snapshot.settings.theme ?? "system");
  const [automaticallyCheckUpdates, setAutomaticallyCheckUpdates] = useState(snapshot.settings.automaticallyCheckUpdates ?? true);
  const [automaticallyDownloadUpdates, setAutomaticallyDownloadUpdates] = useState(snapshot.settings.automaticallyDownloadUpdates ?? false);
  const [includePrereleaseUpdates, setIncludePrereleaseUpdates] = useState(snapshot.settings.includePrereleaseUpdates ?? true);
  const [preferredRecordingApp, setPreferredRecordingApp] = useState(snapshot.settings.preferredRecordingApp ?? "");
  const [automaticallyArchiveRecordings, setAutomaticallyArchiveRecordings] = useState(snapshot.settings.automaticallyArchiveRecordings ?? false);
  const [automaticallyArchiveClips, setAutomaticallyArchiveClips] = useState(snapshot.settings.automaticallyArchiveClips ?? false);
  const [keepLocalClips, setKeepLocalClips] = useState(snapshot.settings.keepLocalClips ?? true);
  const [pauseUploadsWhileRecording, setPauseUploadsWhileRecording] = useState(snapshot.settings.pauseUploadsWhileRecording ?? true);
  const [youtubeBusy, setYouTubeBusy] = useState(false);
  const [maintenanceStatus, setMaintenanceStatus] = useState<string | null>(null);
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
    setTheme(snapshot.settings.theme ?? "system");
    setAutomaticallyCheckUpdates(snapshot.settings.automaticallyCheckUpdates ?? true);
    setAutomaticallyDownloadUpdates(snapshot.settings.automaticallyDownloadUpdates ?? false);
    setIncludePrereleaseUpdates(snapshot.settings.includePrereleaseUpdates ?? true);
    setPreferredRecordingApp(snapshot.settings.preferredRecordingApp ?? "");
    setAutomaticallyArchiveRecordings(snapshot.settings.automaticallyArchiveRecordings ?? false);
    setAutomaticallyArchiveClips(snapshot.settings.automaticallyArchiveClips ?? false);
    setKeepLocalClips(snapshot.settings.keepLocalClips ?? true);
    setPauseUploadsWhileRecording(snapshot.settings.pauseUploadsWhileRecording ?? true);
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
        cleanupThresholdPercent: Math.max(50, Math.min(99, Number(threshold) || 90)),
        theme,
        automaticallyCheckUpdates,
        automaticallyDownloadUpdates,
        includePrereleaseUpdates,
        preferredRecordingApp: preferredRecordingApp || null,
        automaticallyArchiveRecordings,
        automaticallyArchiveClips,
        keepLocalClips,
        pauseUploadsWhileRecording
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
          <AppIcon name="add" size={17} />
          Add Source Folder
        </button>
        <label>
          Working folder
          <input value={workingFolder} onChange={(event) => setWorkingFolder(event.target.value)} placeholder="Optional" />
        </label>
        <button className="secondary-button" onClick={chooseWorkingFolder} title="Choose working folder">
          <AppIcon name="add" size={17} />
          Choose Working Folder
        </button>
        <label>
          Clip folder
          <input value={clipFolder} onChange={(event) => setClipFolder(event.target.value)} placeholder="Defaults to working folder" />
        </label>
        <button className="secondary-button" onClick={chooseClipFolder} title="Choose clip folder">
          <AppIcon name="add" size={17} />
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
        <h2>Appearance</h2>
        <label>
          Theme
          <select value={theme} onChange={(event) => setTheme(event.target.value as "system" | "dark" | "light")}>
            <option value="system">Use Windows setting</option>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
          </select>
        </label>
        <p className="muted">Icons and status treatments retain labels and contrast in both themes.</p>
      </section>
      <section className="settings-panel">
        <h2>Updates</h2>
        <label className="toggle-row"><input type="checkbox" checked={automaticallyCheckUpdates} onChange={(event) => setAutomaticallyCheckUpdates(event.target.checked)} /><span>Automatically check after startup</span></label>
        <label className="toggle-row"><input type="checkbox" checked={automaticallyDownloadUpdates} onChange={(event) => setAutomaticallyDownloadUpdates(event.target.checked)} /><span>Download signed updates automatically</span></label>
        <label className="toggle-row"><input type="checkbox" checked={includePrereleaseUpdates} onChange={(event) => setIncludePrereleaseUpdates(event.target.checked)} /><span>Include alpha and prerelease versions</span></label>
        <p className="muted">Updates are verified with Reelspace's Tauri signing key before installation. Active FFmpeg work is never interrupted.</p>
      </section>
      <section className="settings-panel">
        <h2>Recorder</h2>
        <label>Preferred recording application<select value={preferredRecordingApp} onChange={(event) => setPreferredRecordingApp(event.target.value)}><option value="">Use first detected app</option>{recorders.map((recorder) => <option key={recorder.id} value={recorder.executablePath}>{recorder.name}</option>)}</select></label>
        <label className="toggle-row"><input type="checkbox" checked={pauseUploadsWhileRecording} onChange={(event) => setPauseUploadsWhileRecording(event.target.checked)} /><span>Pause archive uploads while recording</span></label>
        <p className="muted">Recorder launch is a separate local shortcut and never changes archive state.</p>
      </section>
      <section className="settings-panel">
        <h2>Archive defaults</h2>
        <label className="toggle-row"><input type="checkbox" checked={automaticallyArchiveRecordings} onChange={(event) => setAutomaticallyArchiveRecordings(event.target.checked)} /><span>Automatically queue new recordings</span></label>
        <label className="toggle-row"><input type="checkbox" checked={automaticallyArchiveClips} onChange={(event) => setAutomaticallyArchiveClips(event.target.checked)} /><span>Automatically queue generated clips</span></label>
        <label className="toggle-row"><input type="checkbox" checked={keepLocalClips} onChange={(event) => setKeepLocalClips(event.target.checked)} /><span>Keep generated clips local by default</span></label>
        <p className="muted">Automatic local cleanup remains disabled. Only verified archives can become cleanup candidates.</p>
      </section>
      <section className="settings-panel archive-account-panel">
        <h2>YouTube archive</h2>
        {archiveAccount ? <><div className="account-state"><AppIcon name="archive" size={18} /><div><strong>{archiveAccount.displayName}</strong><span>Connected · private uploads only</span></div></div><button className="secondary-button" disabled={youtubeBusy} onClick={async () => { setYouTubeBusy(true); setError(null); try { await onDisconnectYouTube(); } catch (reason) { setError(normalizeAppError(reason).message); } finally { setYouTubeBusy(false); } }}>Disconnect YouTube</button></> : <>
          <p className="muted">Connect your Google account in the browser. Reelspace requests access only for private YouTube archival and stores your sign-in tokens in Windows Credential Manager.</p>
          <button className="primary-button" disabled={youtubeBusy} onClick={async () => { setYouTubeBusy(true); setError(null); try { await onConnectYouTube(); } catch (reason) { setError(normalizeAppError(reason).message); } finally { setYouTubeBusy(false); } }}><AppIcon name="archive" size={17} />{youtubeBusy ? "Waiting for Google…" : "Connect YouTube"}</button>
        </>}
      </section>
      <section className="settings-panel">
        <h2>Library data</h2>
        <p className="muted">Move tags, notes, collections, and clip references without copying video, thumbnails, or previews.</p>
        <button className="secondary-button" onClick={exportData} disabled={dataBusy !== null} title="Export portable library data">
          <AppIcon name="download" size={17} />
          {dataBusy === "export" ? "Exporting..." : "Export Library Data"}
        </button>
        <button className="secondary-button" onClick={importData} disabled={dataBusy !== null} title="Import portable library data">
          <AppIcon name="upload" size={17} />
          {dataBusy === "import" ? "Importing..." : "Import Library Data"}
        </button>
        {dataStatus ? <p className="data-status">{dataStatus}</p> : null}
      </section>
      <section className="settings-panel">
        <h2>About</h2>
        <div className="about-brand"><span><FrameStackMark size={44} /></span><div><strong>Reelspace {snapshot.version}</strong><small>Frame Stack alpha</small></div></div>
        <p className="muted">Local-first Windows alpha. No analytics or automatic crash uploads.</p>
      </section>
      <section className="settings-panel">
        <h2>Diagnostics and repair</h2>
        <button className="secondary-button" onClick={() => api.openDiagnosticsFolder().catch((reason) => setError(normalizeAppError(reason).message))}><AppIcon name="search" size={17} /> Open log folder</button>
        <button className="secondary-button" onClick={async () => { const path = await pickDiagnosticsExport(); if (!path) return; try { await api.exportDiagnostics(path); setMaintenanceStatus("Sanitized diagnostics exported"); } catch (reason) { setError(normalizeAppError(reason).message); } }}><AppIcon name="download" size={17} /> Export diagnostic report</button>
        <button className="secondary-button" onClick={async () => { setMaintenanceStatus("Repairing library…"); try { await api.scanSources(); await api.repairThumbnails(); const summary = await api.repairLibrary(); setMaintenanceStatus(`Repair complete: ${summary.missingLocalFiles} missing local, ${summary.preservedRemoteOnlyItems} preserved remote-only.`); } catch (reason) { setError(normalizeAppError(reason).message); setMaintenanceStatus(null); } }}><AppIcon name="repair" size={17} /> Refresh and repair library</button>
        {maintenanceStatus ? <p className="data-status">{maintenanceStatus}</p> : null}
      </section>
      <div className="settings-actions">
        {error ? <div className="inline-error">{error}</div> : null}
        <button className="primary-button" onClick={save} disabled={busy} title="Save settings">
          <AppIcon name="save" size={17} />
          {busy ? "Saving..." : "Save Settings"}
        </button>
      </div>
    </div>
  );
}
