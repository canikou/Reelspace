import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AppIcon } from "./components/icons";
import { CollectionPickerDialog } from "./components/CollectionPickerDialog";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { GamePickerDialog } from "./components/GamePickerDialog";
import { PlayerModal } from "./components/PlayerModal";
import { RecorderToggle } from "./components/RecorderToggle";
import { SetupPrompt } from "./components/SetupPrompt";
import { Sidebar, type PageKey } from "./components/Sidebar";
import { TextEditorDialog } from "./components/TextEditorDialog";
import { TrimModal } from "./components/TrimModal";
import { UpdatePrompt } from "./components/UpdatePrompt";
import { api, normalizeAppError } from "./lib/api";
import { pickFolder } from "./lib/dialog";
import { formatBytes } from "./lib/format";
import type { AppSnapshot, ArchiveAccount, Collection, Game, MediaItem, RecorderApp, UploadJob } from "./lib/types";
import { checkForUpdate, dismissUpdate, downloadUpdate, installUpdate, type UpdateViewState } from "./lib/updater";
import { ClipsPage } from "./pages/ClipsPage";
import { CollectionsPage } from "./pages/CollectionsPage";
import { GalleryPage } from "./pages/GalleryPage";
import { HomePage } from "./pages/HomePage";
import { SettingsPage } from "./pages/SettingsPage";
import { GamesPage } from "./pages/GamesPage";
import { ArchivePage } from "./pages/ArchivePage";

type EditorState = { kind: "tags" | "notes"; item: MediaItem } | null;

export function App() {
  const [page, setPage] = useState<PageKey>("home");
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [rawItems, setRawItems] = useState<MediaItem[]>([]);
  const [clipItems, setClipItems] = useState<MediaItem[]>([]);
  const [collections, setCollections] = useState<Collection[]>([]);
  const [games, setGames] = useState<Game[]>([]);
  const [uploadJobs, setUploadJobs] = useState<UploadJob[]>([]);
  const [recorders, setRecorders] = useState<RecorderApp[]>([]);
  const [archiveAccount, setArchiveAccount] = useState<ArchiveAccount | null>(null);
  const [playing, setPlaying] = useState<MediaItem | null>(null);
  const [trimming, setTrimming] = useState<MediaItem | null>(null);
  const [editing, setEditing] = useState<EditorState>(null);
  const [collectionTarget, setCollectionTarget] = useState<MediaItem | null>(null);
  const [gameTarget, setGameTarget] = useState<MediaItem | null>(null);
  const [creatingCollection, setCreatingCollection] = useState(false);
  const [selectedCollection, setSelectedCollection] = useState<Collection | null>(null);
  const [collectionItems, setCollectionItems] = useState<MediaItem[]>([]);
  const [highlightedClipId, setHighlightedClipId] = useState<string | null>(null);
  const [pendingDelete, setPendingDelete] = useState<MediaItem | null>(null);
  const [pendingLocalRemoval, setPendingLocalRemoval] = useState<MediaItem | null>(null);
  const [pendingRemoteRemoval, setPendingRemoteRemoval] = useState<MediaItem | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [setupDismissed, setSetupDismissed] = useState(false);
  const [updateState, setUpdateState] = useState<UpdateViewState>({ phase: "idle" });
  const scanInFlight = useRef(false);
  const lastScanAt = useRef(0);
  const refreshGeneration = useRef(0);
  const collectionRequestGeneration = useRef(0);

  const showError = useCallback((value: unknown) => {
    const normalized = normalizeAppError(value);
    if (normalized.code !== "mediaProcessCancelled" && normalized.code !== "exportCancelled") {
      setError(normalized.message);
    }
  }, []);

  const refresh = useCallback(async () => {
    const generation = ++refreshGeneration.current;
    const results = await Promise.allSettled([
      api.getAppState(),
      api.listMedia("raw"),
      api.listMedia("clip"),
      api.listCollections(),
      api.listArchiveStatuses(),
      api.listGameAssignments(),
      api.listGames(),
      api.listUploadJobs(),
      api.detectRecordingApps(),
      api.getArchiveAccount()
    ] as const);
    if (generation !== refreshGeneration.current) return;
    const failures: unknown[] = [];
    if (results[0].status === "fulfilled") setSnapshot(results[0].value); else failures.push(results[0].reason);
    const statuses = results[4].status === "fulfilled" ? new Map(results[4].value.map((status) => [status.mediaId, status])) : new Map();
    const assignments = results[5].status === "fulfilled" ? new Map(results[5].value.map((assignment) => [assignment.mediaId, assignment])) : new Map();
    const enrich = (items: MediaItem[]) => items.map((item) => {
      const status = statuses.get(item.id);
      const game = assignments.get(item.id);
      return { ...item,
        availability: status?.state ?? "localOnly",
        keepLocal: status?.keepLocal ?? item.kind === "clip",
        remoteCopyCount: status?.remoteCopyCount ?? 0,
        primaryRemoteUrl: status?.primaryRemoteUrl ?? null,
        gameId: game?.gameId ?? null,
        gameName: game?.gameName ?? null
      };
    });
    if (results[1].status === "fulfilled") setRawItems(enrich(results[1].value)); else failures.push(results[1].reason);
    if (results[2].status === "fulfilled") setClipItems(enrich(results[2].value)); else failures.push(results[2].reason);
    if (results[3].status === "fulfilled") setCollections(results[3].value); else failures.push(results[3].reason);
    if (results[4].status === "rejected") failures.push(results[4].reason);
    if (results[5].status === "rejected") failures.push(results[5].reason);
    if (results[6].status === "fulfilled") setGames(results[6].value); else failures.push(results[6].reason);
    if (results[7].status === "fulfilled") setUploadJobs(results[7].value); else failures.push(results[7].reason);
    if (results[8].status === "fulfilled") setRecorders(results[8].value); else failures.push(results[8].reason);
    if (results[9].status === "fulfilled") setArchiveAccount(results[9].value); else failures.push(results[9].reason);
    if (failures.length) throw normalizeAppError(failures[0]);
  }, []);

  useEffect(() => {
    refresh().catch(showError);
  }, [refresh, showError]);

  useEffect(() => {
    const theme = snapshot?.settings.theme ?? "system";
    if (theme === "system") delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = theme;
  }, [snapshot?.settings.theme]);

  const runUpdateCheck = useCallback(async (manual = false) => {
    setUpdateState({ phase: "checking" });
    try {
      const next = await checkForUpdate({ manual, includePrerelease: snapshot?.settings.includePrereleaseUpdates ?? true });
      setUpdateState(next);
      if (next.phase === "available" && snapshot?.settings.automaticallyDownloadUpdates) {
        setUpdateState(await downloadUpdate(setUpdateState));
      }
    } catch (reason) {
      const raw = reason instanceof Error ? reason.message : String(reason ?? "");
      const message = /fallback platforms/i.test(raw)
        ? "In-app updates are only available for the installer and AppImage builds. Download the newer package from GitHub Releases."
        : raw || "The update channel could not be reached";
      setUpdateState(manual ? { phase: "error", version: snapshot?.version, error: message } : { phase: "idle" });
    }
  }, [snapshot?.settings.automaticallyDownloadUpdates, snapshot?.settings.includePrereleaseUpdates, snapshot?.version]);

  useEffect(() => {
    if (!snapshot?.settings.automaticallyCheckUpdates) return;
    const timeout = window.setTimeout(() => runUpdateCheck(false), 4_000);
    return () => window.clearTimeout(timeout);
  }, [runUpdateCheck, snapshot?.settings.automaticallyCheckUpdates]);

  const actions = useMemo(
    () => ({
      view: setPlaying,
      trim: (item: MediaItem) => { setPlaying(null); setTrimming(item); },
      reveal: (item: MediaItem) => api.revealInExplorer(item.id).catch(showError),
      notes: (item: MediaItem) => setEditing({ kind: "notes", item }),
      tags: (item: MediaItem) => setEditing({ kind: "tags", item }),
      addToCollection: setCollectionTarget,
      classify: setGameTarget,
      archive: (item: MediaItem) => api.queueArchive([item.id]).then(async () => { await refresh(); setNotice("Queued for private archive"); }).catch(showError),
      setKeepLocal: (item: MediaItem, value: boolean) => api.setKeepLocal(item.id, value).then(async () => { await refresh(); setNotice(value ? "Keep Local enabled" : "Keep Local disabled"); }).catch(showError),
      openRemote: (item: MediaItem) => api.openRemoteCopy(item.id).catch(showError),
      removeLocal: setPendingLocalRemoval,
      removeRemote: setPendingRemoteRemoval
    }),
    [refresh, showError]
  );

  const rootsKey = snapshot?.settings.sourceFolders.join("\0") ?? "";
  const scan = useCallback(async ({ silent = false, force = false }: { silent?: boolean; force?: boolean } = {}) => {
    if (scanInFlight.current) return;
    if (!force && Date.now() - lastScanAt.current < 60_000) return;
    if (!rootsKey) return;
    scanInFlight.current = true;
    if (!silent) setBusy(true);
    setError(null);
    try {
      await api.scanSources();
      lastScanAt.current = Date.now();
      await refresh();
      if (!silent) setNotice("Library is up to date");
    } catch (err) {
      showError(err);
    } finally {
      scanInFlight.current = false;
      if (!silent) setBusy(false);
    }
  }, [refresh, rootsKey, showError]);

  useEffect(() => {
    if (rootsKey) scan({ silent: true, force: true });
  }, [rootsKey, scan]);

  useEffect(() => {
    function maybeScan() {
      if (document.visibilityState === "visible") scan({ silent: true });
    }
    const interval = window.setInterval(maybeScan, 60_000);
    window.addEventListener("focus", maybeScan);
    document.addEventListener("visibilitychange", maybeScan);
    return () => {
      window.clearInterval(interval);
      window.removeEventListener("focus", maybeScan);
      document.removeEventListener("visibilitychange", maybeScan);
    };
  }, [scan]);

  useEffect(() => {
    if (!notice) return;
    const timeout = window.setTimeout(() => setNotice(null), 3000);
    return () => window.clearTimeout(timeout);
  }, [notice]);

  useEffect(() => {
    if (page !== "archive") return;
    const interval = window.setInterval(() => refresh().catch(showError), 5_000);
    return () => window.clearInterval(interval);
  }, [page, refresh, showError]);

  useEffect(() => {
    if (!highlightedClipId) return;
    const timeout = window.setTimeout(() => setHighlightedClipId(null), 5000);
    return () => window.clearTimeout(timeout);
  }, [highlightedClipId]);

  async function chooseInitialSourceFolder() {
    setBusy(true);
    setError(null);
    try {
      const folder = await pickFolder();
      if (!folder || !snapshot) return;
      await api.saveSettings({ ...snapshot.settings, sourceFolders: [folder] });
      setSetupDismissed(true);
      await api.scanSources();
      lastScanAt.current = Date.now();
      await refresh();
    } catch (err) {
      showError(err);
    } finally {
      setBusy(false);
    }
  }

  async function openCollection(collection: Collection) {
    const generation = ++collectionRequestGeneration.current;
    setError(null);
    try {
      const items = await api.listCollectionMedia(collection.id);
      if (generation !== collectionRequestGeneration.current) return;
      setCollectionItems(items);
      setSelectedCollection(collection);
    } catch (err) {
      showError(err);
    }
  }

  return (
    <div className="app-shell">
      <Sidebar page={page} onPageChange={(next) => { setPage(next); if (next !== "collections") setSelectedCollection(null); }} />
      <main className="workspace">
        <header className="topbar">
          <h1>{titleFor(page)}</h1>
          <div className="topbar-actions">
            <RecorderToggle onError={showError} />
            {busy ? <span className="activity-pill"><AppIcon name="refresh" size={14} className="spin" /> Scanning</span> : null}
            {recorders[0] ? <button className="recorder-button" onClick={() => api.launchRecordingApp(recorders[0].id).catch(showError)} aria-label={`Open ${recorders[0].name}`} title={`Open ${recorders[0].name}`}><AppIcon name="recorder" size={17} /><span>{recorders[0].name}</span></button> : null}
            <button className="icon-button" onClick={() => runUpdateCheck(true)} disabled={updateState.phase === "checking" || updateState.phase === "downloading"} aria-label="Check for updates" title="Check for updates">
              <AppIcon name="update" size={18} className={updateState.phase === "checking" ? "spin" : undefined} />
            </button>
            <button className="icon-button" onClick={() => scan({ force: true })} disabled={busy || !rootsKey} aria-label="Refresh media" title="Refresh media">
              <AppIcon name="refresh" size={18} className={busy ? "spin" : undefined} />
            </button>
          </div>
        </header>
        {error ? <div className="error-banner" role="alert"><span>{error}</span>{!snapshot ? <button className="secondary-button" onClick={() => { setError(null); refresh().catch(showError); }}>Try again</button> : null}<button onClick={() => setError(null)} aria-label="Dismiss error" title="Dismiss error"><AppIcon name="close" size={16} /></button></div> : null}
        {notice ? <div className="toast" role="status"><AppIcon name="success" size={17} /> {notice}</div> : null}
        <UpdatePrompt
          state={updateState}
          activeWork={busy || trimming !== null || Boolean(snapshot?.activeMediaWork)}
          onDownload={async () => {
            try { setUpdateState(await downloadUpdate(setUpdateState)); }
            catch (reason) { setUpdateState((current) => ({ ...current, phase: "error", error: reason instanceof Error ? reason.message : "Update download failed" })); }
          }}
          onInstall={() => installUpdate().catch(showError)}
          onLater={() => { dismissUpdate(updateState.version).catch(() => undefined); setUpdateState({ phase: "idle" }); }}
          onSkip={() => { dismissUpdate(updateState.version, true).catch(() => undefined); setUpdateState({ phase: "idle" }); }}
        />
        {!snapshot && !error ? <div className="loading-grid"><i /><i /><i /><i /></div> : null}
        {snapshot && !snapshot.settings.sourceFolders.length && setupDismissed ? (
          <button className="setup-reminder" onClick={chooseInitialSourceFolder}><AppIcon name="add" size={18} /><span>Add a capture folder to begin</span></button>
        ) : null}
        {page === "home" && snapshot ? <HomePage snapshot={snapshot} rawItems={rawItems} clipItems={clipItems} actions={actions} onCleanup={setPendingDelete} /> : null}
        {snapshot && page === "gallery" ? <GalleryPage items={rawItems} actions={actions} /> : null}
        {snapshot && page === "clips" ? <ClipsPage items={clipItems} actions={actions} highlightedId={highlightedClipId} /> : null}
        {snapshot && page === "games" ? <GamesPage games={games} onCreate={async (name) => { setGames(await api.createGame(name)); setNotice("Game created"); }} onRename={async (game, name) => { setGames(await api.renameGame(game.id, name)); await refresh(); setNotice("Game renamed and alias preserved"); }} onMerge={async (source, target) => { setGames(await api.mergeGames(source.id, target.id)); await refresh(); setNotice("Game classifications merged"); }} /> : null}
        {snapshot && page === "collections" ? (
          <CollectionsPage
            collections={collections}
            selected={selectedCollection}
            items={collectionItems}
            actions={actions}
            onCreate={() => setCreatingCollection(true)}
            onSelect={openCollection}
            onBack={() => { collectionRequestGeneration.current += 1; setSelectedCollection(null); setCollectionItems([]); }}
            onRemove={async (collection, item) => { await api.removeFromCollection(collection.id, item.id); await openCollection(collection); await refresh(); setNotice("Removed from collection"); }}
            onStorageDefault={async (collection, policy) => {
              const nextCollections = await api.setCollectionStorageDefault(collection.id, policy);
              setCollections(nextCollections);
              setSelectedCollection(nextCollections.find((item) => item.id === collection.id) ?? null);
              await refresh();
              setNotice(policy === "keepLocal" ? "Collection keeps new items local" : policy === "archive" ? "Collection archives new items" : "Collection uses app defaults");
            }}
          />
        ) : null}
        {snapshot && page === "archive" ? <ArchivePage jobs={uploadJobs} onControl={async (action, jobId) => { setUploadJobs(await api.controlUploadQueue(action, jobId)); await refresh(); }} /> : null}
        {page === "settings" && snapshot ? (
          <SettingsPage
            snapshot={snapshot}
            recorders={recorders}
            archiveAccount={archiveAccount}
            onConnectYouTube={async () => { setArchiveAccount(await api.connectYouTube()); await refresh(); setNotice("YouTube archive connected"); }}
            onDisconnectYouTube={async () => { await api.disconnectYouTube(); setArchiveAccount(null); await refresh(); setNotice("YouTube archive disconnected"); }}
            onSave={async (settings) => {
              const next = await api.saveSettings(settings);
              setSnapshot(next);
              await refresh();
              setNotice("Settings saved");
            }}
            onExportData={api.exportLibraryData}
            onImportData={async (path) => {
              const summary = await api.importLibraryData(path);
              await refresh();
              if (selectedCollection) await openCollection(selectedCollection);
              setNotice("Library data imported");
              return summary;
            }}
          />
        ) : null}
      </main>

      {playing ? <PlayerModal item={playing} actions={actions} onClose={() => setPlaying(null)} /> : null}
      {trimming ? <TrimModal item={trimming} onClose={() => setTrimming(null)} onCreated={async (created) => {
        setTrimming(null);
        setClipItems((current) => [created, ...current.filter((item) => item.id !== created.id)]);
        setHighlightedClipId(created.id);
        setPage("clips");
        setPlaying(created);
        await refresh();
        setNotice("Clip created");
      }} /> : null}
      {editing ? (
        <TextEditorDialog
          title={editing.kind === "tags" ? "Edit tags" : "Edit notes"}
          description={editing.item.filename}
          initialValue={editing.kind === "tags" ? editing.item.tags.join(", ") : editing.item.notes ?? ""}
          multiline={editing.kind === "notes"}
          placeholder={editing.kind === "tags" ? "game, highlight, ranked" : "Add a note"}
          onClose={() => setEditing(null)}
          onSave={async (value) => {
            if (editing.kind === "tags") await api.updateMediaTags(editing.item.id, value.split(",").map((tag) => tag.trim()).filter(Boolean));
            else await api.updateMediaNotes(editing.item.id, value);
            setEditing(null);
            await refresh();
            setNotice(editing.kind === "tags" ? "Tags updated" : "Notes updated");
          }}
        />
      ) : null}
      {collectionTarget && !creatingCollection ? (
        <CollectionPickerDialog
          item={collectionTarget}
          collections={collections}
          onClose={() => setCollectionTarget(null)}
          onCreate={() => setCreatingCollection(true)}
          onPick={async (collection) => {
            await api.addToCollection(collection.id, collectionTarget.id);
            setCollectionTarget(null);
            await refresh();
            setNotice(`Added to ${collection.name}`);
          }}
        />
      ) : null}
      {gameTarget ? (
        <GamePickerDialog
          item={gameTarget}
          games={games}
          onClose={() => setGameTarget(null)}
          onCreate={async (name) => {
            const nextGames = await api.createGame(name);
            setGames(nextGames);
            const created = nextGames.find(
              (game) => game.name.toLocaleLowerCase() === name.trim().toLocaleLowerCase(),
            );
            if (!created) throw new Error("The new game could not be found after creation.");
            return created;
          }}
          onPick={async (game) => {
            await api.assignGame(gameTarget.id, game.id);
            setGameTarget(null);
            await refresh();
            setNotice(`Classified as ${game.name}`);
          }}
        />
      ) : null}
      {creatingCollection ? (
        <TextEditorDialog
          title="New collection"
          placeholder="Collection name"
          onClose={() => setCreatingCollection(false)}
          onSave={async (name) => {
            if (!name.trim()) return;
            setCollections(await api.createCollection(name));
            setCreatingCollection(false);
            setNotice("Collection created");
          }}
        />
      ) : null}
      {pendingDelete ? (
        <ConfirmDialog
          title="Delete raw file?"
          description={`This permanently removes ${formatBytes(pendingDelete.fileSize)} from disk. Clips remain protected.`}
          detail={pendingDelete.path}
          onClose={() => setPendingDelete(null)}
          onConfirm={async () => {
            await api.deleteRawFile(pendingDelete.id);
            setPendingDelete(null);
            await refresh();
            setNotice("Raw file deleted");
          }}
        />
      ) : null}
      {pendingLocalRemoval ? (
        <ConfirmDialog
          title="Remove local copy?"
          description="The verified remote archive will remain available. The local file moves to the system trash."
          detail={pendingLocalRemoval.path}
          confirmLabel="Remove local copy"
          busyLabel="Removing"
          onClose={() => setPendingLocalRemoval(null)}
          onConfirm={async () => { await api.removeLocalCopy(pendingLocalRemoval.id); setPendingLocalRemoval(null); await refresh(); setNotice("Local copy moved to the trash"); }}
        />
      ) : null}
      {pendingRemoteRemoval ? (
        <ConfirmDialog
          title="Delete remote archive?"
          description="This permanently deletes the private YouTube copy. The local file is not changed."
          detail={pendingRemoteRemoval.filename}
          onClose={() => setPendingRemoteRemoval(null)}
          onConfirm={async () => { await api.deleteYouTubeRemote(pendingRemoteRemoval.id); setPendingRemoteRemoval(null); await refresh(); setNotice("Remote copy deleted"); }}
        />
      ) : null}
      {snapshot && !snapshot.settings.sourceFolders.length && !setupDismissed ? (
        <SetupPrompt busy={busy} onChoose={chooseInitialSourceFolder} onDismiss={() => setSetupDismissed(true)} />
      ) : null}
    </div>
  );
}

function titleFor(page: PageKey): string {
  return { home: "Home", gallery: "Recordings", clips: "Clips", games: "Games", collections: "Collections", archive: "Archive", settings: "Settings" }[page];
}
