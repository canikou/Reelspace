import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { CheckCircle2, FolderPlus, RefreshCw, X } from "lucide-react";
import { CollectionPickerDialog } from "./components/CollectionPickerDialog";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { PlayerModal } from "./components/PlayerModal";
import { SetupPrompt } from "./components/SetupPrompt";
import { Sidebar, type PageKey } from "./components/Sidebar";
import { TextEditorDialog } from "./components/TextEditorDialog";
import { TrimModal } from "./components/TrimModal";
import { api, normalizeAppError } from "./lib/api";
import { pickFolder } from "./lib/dialog";
import { formatBytes } from "./lib/format";
import type { AppSnapshot, Collection, MediaItem } from "./lib/types";
import { ClipsPage } from "./pages/ClipsPage";
import { CollectionsPage } from "./pages/CollectionsPage";
import { GalleryPage } from "./pages/GalleryPage";
import { HomePage } from "./pages/HomePage";
import { SettingsPage } from "./pages/SettingsPage";

type EditorState = { kind: "tags" | "notes"; item: MediaItem } | null;

export function App() {
  const [page, setPage] = useState<PageKey>("home");
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [rawItems, setRawItems] = useState<MediaItem[]>([]);
  const [clipItems, setClipItems] = useState<MediaItem[]>([]);
  const [collections, setCollections] = useState<Collection[]>([]);
  const [playing, setPlaying] = useState<MediaItem | null>(null);
  const [trimming, setTrimming] = useState<MediaItem | null>(null);
  const [editing, setEditing] = useState<EditorState>(null);
  const [collectionTarget, setCollectionTarget] = useState<MediaItem | null>(null);
  const [creatingCollection, setCreatingCollection] = useState(false);
  const [selectedCollection, setSelectedCollection] = useState<Collection | null>(null);
  const [collectionItems, setCollectionItems] = useState<MediaItem[]>([]);
  const [highlightedClipId, setHighlightedClipId] = useState<string | null>(null);
  const [pendingDelete, setPendingDelete] = useState<MediaItem | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [setupDismissed, setSetupDismissed] = useState(false);
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
      api.listCollections()
    ] as const);
    if (generation !== refreshGeneration.current) return;
    const failures: string[] = [];
    if (results[0].status === "fulfilled") setSnapshot(results[0].value); else failures.push(String(results[0].reason));
    if (results[1].status === "fulfilled") setRawItems(results[1].value); else failures.push(String(results[1].reason));
    if (results[2].status === "fulfilled") setClipItems(results[2].value); else failures.push(String(results[2].reason));
    if (results[3].status === "fulfilled") setCollections(results[3].value); else failures.push(String(results[3].reason));
    if (failures.length) throw new Error(failures[0]);
  }, []);

  useEffect(() => {
    refresh().catch(showError);
  }, [refresh, showError]);

  const actions = useMemo(
    () => ({
      view: setPlaying,
      trim: (item: MediaItem) => { setPlaying(null); setTrimming(item); },
      reveal: (item: MediaItem) => api.revealInExplorer(item.id).catch(showError),
      notes: (item: MediaItem) => setEditing({ kind: "notes", item }),
      tags: (item: MediaItem) => setEditing({ kind: "tags", item }),
      addToCollection: setCollectionTarget
    }),
    [showError]
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
          <div>
            <p className="eyebrow">Reelspace</p>
            <h1>{titleFor(page)}</h1>
          </div>
          <div className="topbar-actions">
            {busy ? <span className="activity-pill"><RefreshCw size={14} className="spin" /> Scanning</span> : null}
            <button className="icon-button" onClick={() => scan({ force: true })} disabled={busy || !rootsKey} aria-label="Refresh media" title="Refresh media">
              <RefreshCw size={18} className={busy ? "spin" : undefined} />
            </button>
          </div>
        </header>
        {error ? <div className="error-banner"><span>{error}</span><button onClick={() => setError(null)} aria-label="Dismiss error"><X size={16} /></button></div> : null}
        {notice ? <div className="toast"><CheckCircle2 size={17} /> {notice}</div> : null}
        {!snapshot && !error ? <div className="loading-grid"><i /><i /><i /><i /></div> : null}
        {snapshot && !snapshot.settings.sourceFolders.length && setupDismissed ? (
          <button className="setup-reminder" onClick={chooseInitialSourceFolder}><FolderPlus size={18} /><span>Add a capture folder to begin</span></button>
        ) : null}
        {page === "home" && snapshot ? <HomePage snapshot={snapshot} rawItems={rawItems} clipItems={clipItems} actions={actions} onCleanup={setPendingDelete} /> : null}
        {page === "gallery" ? <GalleryPage items={rawItems} actions={actions} /> : null}
        {page === "clips" ? <ClipsPage items={clipItems} actions={actions} highlightedId={highlightedClipId} /> : null}
        {page === "collections" ? (
          <CollectionsPage
            collections={collections}
            selected={selectedCollection}
            items={collectionItems}
            actions={actions}
            onCreate={() => setCreatingCollection(true)}
            onSelect={openCollection}
            onBack={() => { collectionRequestGeneration.current += 1; setSelectedCollection(null); setCollectionItems([]); }}
          />
        ) : null}
        {page === "settings" && snapshot ? (
          <SettingsPage snapshot={snapshot} onSave={async (settings) => {
            const next = await api.saveSettings(settings);
            setSnapshot(next);
            await refresh();
            setNotice("Settings saved");
          }} />
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
      {snapshot && !snapshot.settings.sourceFolders.length && !setupDismissed ? (
        <SetupPrompt busy={busy} onChoose={chooseInitialSourceFolder} onDismiss={() => setSetupDismissed(true)} />
      ) : null}
    </div>
  );
}

function titleFor(page: PageKey): string {
  return { home: "Home", gallery: "Gallery", clips: "Clips", collections: "Collections", settings: "Settings" }[page];
}
