import { useEffect, useRef, useState } from "react";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { RecorderSections, useRecorderSettings } from "../components/RecorderSettings";
import { SettingRow, SettingsGroup, SettingsSection } from "../components/SettingRow";
import { AppIcon, FrameStackMark } from "../components/icons";
import { api, normalizeAppError } from "../lib/api";
import { pickDiagnosticsExport, pickFolder, pickLibraryDataExport, pickLibraryDataImport } from "../lib/dialog";
import { formatBytes } from "../lib/format";
import { pickActiveSection } from "../lib/settingsNav";
import type { AppSettings, AppSnapshot, ArchiveAccount, LibraryDataExportSummary, LibraryDataImportSummary, RecorderApp } from "../lib/types";

const GB = 1024 * 1024 * 1024;

type Values = {
  sourceFolders: string; workingFolder: string; clipFolder: string; limitGb: string; threshold: string;
  theme: "system" | "dark" | "light";
  checkUpdates: boolean; downloadUpdates: boolean; prerelease: boolean;
  recordingApp: string; archiveRecordings: boolean; archiveClips: boolean; keepLocalClips: boolean; pauseUploads: boolean;
};

const fromSettings = (s: AppSettings): Values => ({
  sourceFolders: s.sourceFolders.join("\n"),
  workingFolder: s.workingFolder ?? "",
  clipFolder: s.clipFolder ?? "",
  limitGb: s.storageLimitBytes ? String(Math.round(s.storageLimitBytes / GB)) : "",
  threshold: String(s.cleanupThresholdPercent),
  theme: s.theme ?? "system",
  checkUpdates: s.automaticallyCheckUpdates ?? true,
  downloadUpdates: s.automaticallyDownloadUpdates ?? false,
  prerelease: s.includePrereleaseUpdates ?? true,
  recordingApp: s.preferredRecordingApp ?? "",
  archiveRecordings: s.automaticallyArchiveRecordings ?? false,
  archiveClips: s.automaticallyArchiveClips ?? false,
  keepLocalClips: s.keepLocalClips ?? true,
  pauseUploads: s.pauseUploadsWhileRecording ?? true,
});

const toSettings = (v: Values): AppSettings => ({
  sourceFolders: v.sourceFolders.split(/\r?\n/).map((line) => line.trim()).filter(Boolean),
  workingFolder: v.workingFolder.trim() || null,
  clipFolder: v.clipFolder.trim() || null,
  storageLimitBytes: v.limitGb ? Number(v.limitGb) * GB : null,
  cleanupThresholdPercent: Math.max(50, Math.min(99, Number(v.threshold) || 90)),
  theme: v.theme,
  automaticallyCheckUpdates: v.checkUpdates,
  automaticallyDownloadUpdates: v.downloadUpdates,
  includePrereleaseUpdates: v.prerelease,
  preferredRecordingApp: v.recordingApp || null,
  automaticallyArchiveRecordings: v.archiveRecordings,
  automaticallyArchiveClips: v.archiveClips,
  keepLocalClips: v.keepLocalClips,
  pauseUploadsWhileRecording: v.pauseUploads,
});

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
  const [v, setV] = useState<Values>(() => fromSettings(snapshot.settings));
  const up = (patch: Partial<Values>) => setV((current) => ({ ...current, ...patch }));
  const [youtubeBusy, setYouTubeBusy] = useState(false);
  const [maintenanceStatus, setMaintenanceStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dataBusy, setDataBusy] = useState<"export" | "import" | null>(null);
  const [dataStatus, setDataStatus] = useState<string | null>(null);
  const [pendingImport, setPendingImport] = useState<string | null>(null);
  const [recorderSupported, setRecorderSupported] = useState(false);
  const [active, setActive] = useState("general");
  const saveInFlight = useRef(false);
  const recorder = useRecorderSettings(recorderSupported);

  const appDirty = JSON.stringify(toSettings(v)) !== JSON.stringify(toSettings(fromSettings(snapshot.settings)));
  const dirty = appDirty || recorder.dirty;

  const sections = [
    { id: "general", label: "General" },
    { id: "library", label: "Library & folders" },
    ...(recorderSupported ? [{ id: "recorder", label: "Recorder" }] : []),
    { id: "archive", label: "Archive & YouTube" },
    { id: "updates", label: "Updates" },
    { id: "advanced", label: "Advanced" },
  ];
  const sectionKey = sections.map((s) => s.id).join(",");

  useEffect(() => {
    api.recorderSupported().then(setRecorderSupported).catch(() => undefined);
  }, []);

  useEffect(() => { setV(fromSettings(snapshot.settings)); }, [snapshot]);

  // Highlight the section in the reading band near the top of the scroll area.
  useEffect(() => {
    if (typeof IntersectionObserver === "undefined") return;
    const order = sectionKey.split(",");
    const visible = new Set<string>();
    const observer = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        const id = (entry.target as HTMLElement).dataset.section!;
        if (entry.isIntersecting) visible.add(id); else visible.delete(id);
      }
      setActive(pickActiveSection(order, visible, order[0]));
    }, { rootMargin: "-120px 0px -65% 0px" });
    for (const id of order) {
      const el = document.getElementById(`settings-${id}`);
      if (el) observer.observe(el);
    }
    return () => observer.disconnect();
  }, [sectionKey]);

  function goTo(id: string) {
    setActive(id);
    const reduce = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    document.getElementById(`settings-${id}`)?.scrollIntoView({ behavior: reduce ? "auto" : "smooth", block: "start" });
  }

  async function save() {
    if (saveInFlight.current) return;
    saveInFlight.current = true;
    setBusy(true);
    setError(null);
    try {
      if (recorder.dirty && !(await recorder.save())) return;
      if (appDirty) await onSave(toSettings(v));
    } catch (reason) {
      setError(normalizeAppError(reason).message);
    } finally {
      saveInFlight.current = false;
      setBusy(false);
    }
  }

  async function addSourceFolder() {
    const folder = await pickFolder();
    if (!folder) return;
    const existing = v.sourceFolders.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
    if (!existing.some((value) => value.toLowerCase() === folder.toLowerCase())) {
      up({ sourceFolders: [...existing, folder].join("\n") });
    }
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
      setPendingImport(path);
    } catch (reason) {
      setError(normalizeAppError(reason).message);
    }
  }

  async function runImport(path: string) {
    setError(null);
    setDataStatus(null);
    try {
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

  async function youtube(action: () => Promise<unknown>) {
    setYouTubeBusy(true);
    setError(null);
    try { await action(); } catch (reason) { setError(normalizeAppError(reason).message); } finally { setYouTubeBusy(false); }
  }

  const toggle = (id: string, label: string, help: string | undefined, key: keyof Values) => (
    <SettingRow label={label} help={help} htmlFor={id}>
      <input id={id} type="checkbox" role="switch" checked={v[key] as boolean} onChange={(event) => up({ [key]: event.target.checked } as Partial<Values>)} />
    </SettingRow>
  );
  const shownError = error ?? recorder.error;

  return (
    <div className="settings-shell">
      <nav className="settings-nav" aria-label="Settings categories">
        {sections.map((s) => (
          <button key={s.id} type="button" className={active === s.id ? "active" : undefined} aria-current={active === s.id ? "true" : undefined} onClick={() => goTo(s.id)}>{s.label}</button>
        ))}
      </nav>
      <div className="settings-main">
        <SettingsSection id="general" title="General">
          <SettingRow label="Theme" help="Icons and status treatments keep labels and contrast in both themes." htmlFor="set-theme">
            <select id="set-theme" value={v.theme} onChange={(event) => up({ theme: event.target.value as Values["theme"] })}>
              <option value="system">Use system setting</option>
              <option value="dark">Dark</option>
              <option value="light">Light</option>
            </select>
          </SettingRow>
          <SettingRow label="Preferred recording application" help="Used by the launcher button in the header. It never changes archive state." htmlFor="set-recording-app">
            <select id="set-recording-app" value={v.recordingApp} onChange={(event) => up({ recordingApp: event.target.value })}>
              <option value="">Use first detected app</option>
              {recorders.map((recorder) => <option key={recorder.id} value={recorder.executablePath}>{recorder.name}</option>)}
            </select>
          </SettingRow>
          <SettingRow label="About">
            <div className="about-brand"><span><FrameStackMark size={34} /></span><div><strong>Reelspace {snapshot.version}</strong><small>Frame Stack alpha · local-first, no analytics or crash uploads</small></div></div>
          </SettingRow>
        </SettingsSection>

        <SettingsSection id="library" title="Library & folders">
          <SettingsGroup title="Folders">
            <SettingRow label="Source folders" help="One folder per line. Reelspace scans these for recordings." htmlFor="set-sources" stack>
              <textarea id="set-sources" value={v.sourceFolders} onChange={(event) => up({ sourceFolders: event.target.value })} rows={4} />
              <button className="secondary-button" onClick={addSourceFolder} title="Add a source folder"><AppIcon name="add" size={17} />Add source folder</button>
            </SettingRow>
            <SettingRow label="Working folder" help="Optional." htmlFor="set-working" stack>
              <div className="inline-pick">
                <input id="set-working" value={v.workingFolder} onChange={(event) => up({ workingFolder: event.target.value })} />
                <button className="secondary-button" onClick={async () => { const f = await pickFolder(); if (f) up({ workingFolder: f }); }} title="Choose working folder">Choose</button>
              </div>
            </SettingRow>
            <SettingRow label="Clip folder" help="Defaults to the working folder." htmlFor="set-clips" stack>
              <div className="inline-pick">
                <input id="set-clips" value={v.clipFolder} onChange={(event) => up({ clipFolder: event.target.value })} />
                <button className="secondary-button" onClick={async () => { const f = await pickFolder(); if (f) up({ clipFolder: f }); }} title="Choose clip folder">Choose</button>
              </div>
            </SettingRow>
          </SettingsGroup>
          <SettingsGroup title="Storage">
            <SettingRow label="Storage limit (GB)" help={`Optional. Current managed usage: ${formatBytes(snapshot.stats.managedBytes)}.`} htmlFor="set-limit">
              <input id="set-limit" type="number" min="1" value={v.limitGb} onChange={(event) => up({ limitGb: event.target.value })} inputMode="numeric" />
            </SettingRow>
            <SettingRow label="Cleanup warning threshold (%)" help="Warn when managed storage passes this share of the limit." htmlFor="set-threshold">
              <input id="set-threshold" type="number" min="50" max="99" value={v.threshold} onChange={(event) => up({ threshold: event.target.value })} inputMode="numeric" />
            </SettingRow>
          </SettingsGroup>
          <SettingsGroup title="Library data">
            <SettingRow label="Export and import" help="Move tags, notes, collections, and clip references without copying video, thumbnails, or previews.">
              <div className="set-buttons">
                <button className="secondary-button" onClick={exportData} disabled={dataBusy !== null} title="Export portable library data"><AppIcon name="download" size={17} />{dataBusy === "export" ? "Exporting..." : "Export Library Data"}</button>
                <button className="secondary-button" onClick={importData} disabled={dataBusy !== null} title="Import portable library data"><AppIcon name="upload" size={17} />{dataBusy === "import" ? "Importing..." : "Import Library Data"}</button>
              </div>
            </SettingRow>
            {dataStatus ? <p className="data-status" role="status">{dataStatus}</p> : null}
          </SettingsGroup>
        </SettingsSection>

        {recorderSupported ? (
          <SettingsSection id="recorder" title="Recorder">
            <RecorderSections r={recorder} />
          </SettingsSection>
        ) : null}

        <SettingsSection id="archive" title="Archive & YouTube">
          <SettingsGroup title="Archive defaults">
            {toggle("set-arch-rec", "Automatically queue new recordings", undefined, "archiveRecordings")}
            {toggle("set-arch-clip", "Automatically queue generated clips", undefined, "archiveClips")}
            {toggle("set-keep-local", "Keep generated clips local by default", "Automatic local cleanup remains disabled. Only verified archives can become cleanup candidates.", "keepLocalClips")}
            {toggle("set-pause-uploads", "Pause archive uploads while recording", undefined, "pauseUploads")}
          </SettingsGroup>
          <SettingsGroup title="YouTube archive">
            {archiveAccount ? (
              <SettingRow label={archiveAccount.displayName} help="Connected · private uploads only">
                <button className="secondary-button" disabled={youtubeBusy} onClick={() => youtube(onDisconnectYouTube)}>Disconnect YouTube</button>
              </SettingRow>
            ) : (
              <SettingRow label="YouTube account" help="Connect your Google account in the browser. Reelspace requests access only for private YouTube archival and stores your sign-in tokens in the system credential store.">
                <button className="primary-button" disabled={youtubeBusy} onClick={() => youtube(onConnectYouTube)}><AppIcon name="archive" size={17} />{youtubeBusy ? "Waiting for Google…" : "Connect YouTube"}</button>
              </SettingRow>
            )}
          </SettingsGroup>
        </SettingsSection>

        <SettingsSection id="updates" title="Updates">
          {toggle("set-check", "Automatically check after startup", undefined, "checkUpdates")}
          {toggle("set-download", "Download signed updates automatically", "Updates are verified with Reelspace's Tauri signing key before installation. Active FFmpeg work is never interrupted.", "downloadUpdates")}
          {toggle("set-pre", "Include alpha and prerelease versions", undefined, "prerelease")}
        </SettingsSection>

        <SettingsSection id="advanced" title="Advanced">
          <SettingsGroup title="Tools">
            <SettingRow label={snapshot.ffmpeg.ready ? "FFmpeg ready" : "FFmpeg missing"} help={snapshot.ffmpeg.message}>
              <details className="tool-paths">
                <summary>Tool paths</summary>
                <code>{snapshot.ffmpeg.ffmpegPath ?? "ffmpeg not found"}</code>
                <code>{snapshot.ffmpeg.ffprobePath ?? "ffprobe not found"}</code>
              </details>
            </SettingRow>
          </SettingsGroup>
          <SettingsGroup title="Diagnostics and repair">
            <SettingRow label="Log folder" help="Open the folder that holds Reelspace logs.">
              <button className="secondary-button" onClick={() => api.openDiagnosticsFolder().catch((reason) => setError(normalizeAppError(reason).message))}><AppIcon name="search" size={17} /> Open log folder</button>
            </SettingRow>
            <SettingRow label="Diagnostic report" help="Export a sanitized report you can share.">
              <button className="secondary-button" onClick={async () => { const path = await pickDiagnosticsExport(); if (!path) return; try { await api.exportDiagnostics(path); setMaintenanceStatus("Sanitized diagnostics exported"); } catch (reason) { setError(normalizeAppError(reason).message); } }}><AppIcon name="download" size={17} /> Export diagnostic report</button>
            </SettingRow>
            <SettingRow label="Repair library" help="Rescan sources, rebuild thumbnails, and reconcile missing files.">
              <button className="secondary-button" onClick={async () => { setMaintenanceStatus("Repairing library…"); try { await api.scanSources(); await api.repairThumbnails(); const summary = await api.repairLibrary(); setMaintenanceStatus(`Repair complete: ${summary.missingLocalFiles} missing local, ${summary.preservedRemoteOnlyItems} preserved remote-only.`); } catch (reason) { setError(normalizeAppError(reason).message); setMaintenanceStatus(null); } }}><AppIcon name="repair" size={17} /> Refresh and repair library</button>
            </SettingRow>
            {maintenanceStatus ? <p className="data-status" role="status">{maintenanceStatus}</p> : null}
          </SettingsGroup>
        </SettingsSection>

        {pendingImport ? (
          <ConfirmDialog
            title="Import library data?"
            description="Tags and collection membership will be merged. Existing notes are preserved when they conflict."
            detail={pendingImport}
            tone="neutral"
            confirmLabel="Import"
            busyLabel="Importing"
            onClose={() => setPendingImport(null)}
            onConfirm={async () => { const path = pendingImport; setPendingImport(null); await runImport(path); }}
          />
        ) : null}
        <div className="settings-actions">
          {shownError ? <div className="inline-error" role="alert">{shownError}</div> : null}
          {dirty ? <span className="unsaved" role="status">Unsaved changes</span> : null}
          <button className="primary-button" onClick={save} disabled={busy || !dirty} title="Save settings">
            <AppIcon name="save" size={17} />
            {busy ? "Saving..." : "Save Settings"}
          </button>
        </div>
      </div>
    </div>
  );
}
