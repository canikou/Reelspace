import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { RefreshCw } from "lucide-react";
import { Sidebar, type PageKey } from "./components/Sidebar";
import { PlayerModal } from "./components/PlayerModal";
import { SetupPrompt } from "./components/SetupPrompt";
import { TrimModal } from "./components/TrimModal";
import { api } from "./lib/api";
import { pickFolder } from "./lib/dialog";
import type { AppSnapshot, Collection, MediaItem } from "./lib/types";
import { ClipsPage } from "./pages/ClipsPage";
import { CollectionsPage } from "./pages/CollectionsPage";
import { GalleryPage } from "./pages/GalleryPage";
import { HomePage } from "./pages/HomePage";
import { SettingsPage } from "./pages/SettingsPage";

export function App() {
  const [page, setPage] = useState<PageKey>("home");
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [rawItems, setRawItems] = useState<MediaItem[]>([]);
  const [clipItems, setClipItems] = useState<MediaItem[]>([]);
  const [collections, setCollections] = useState<Collection[]>([]);
  const [playing, setPlaying] = useState<MediaItem | null>(null);
  const [trimming, setTrimming] = useState<MediaItem | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [setupDismissed, setSetupDismissed] = useState(false);
  const scanInFlight = useRef(false);
  const lastScanAt = useRef(0);

  const refresh = useCallback(async () => {
    setError(null);
    const [nextSnapshot, raws, clips, nextCollections] = await Promise.all([
      api.getAppState(),
      api.listMedia("raw"),
      api.listMedia("clip"),
      api.listCollections()
    ]);
    setSnapshot(nextSnapshot);
    setRawItems(raws);
    setClipItems(clips);
    setCollections(nextCollections);
  }, []);

  useEffect(() => {
    refresh().catch((err) => setError(String(err)));
  }, [refresh]);

  const actions = useMemo(
    () => ({
      view: setPlaying,
      trim: setTrimming,
      reveal: (item: MediaItem) => api.revealInExplorer(item.path).catch((err) => setError(String(err))),
      notes: async (item: MediaItem) => {
        const notes = window.prompt("Notes", item.notes ?? "");
        if (notes === null) return;
        await api.updateMediaNotes(item.id, notes);
        await refresh();
      },
      tags: async (item: MediaItem) => {
        const tags = window.prompt("Tags, separated by commas", item.tags.join(", "));
        if (tags === null) return;
        await api.updateMediaTags(
          item.id,
          tags.split(",").map((tag) => tag.trim()).filter(Boolean)
        );
        await refresh();
      },
      addToCollection: async (item: MediaItem) => {
        let collectionId = collections[0]?.id;
        if (!collectionId) {
          const name = window.prompt("New collection name", "Favorites");
          if (!name) return;
          const next = await api.createCollection(name);
          collectionId = next[0]?.id;
        }
        if (!collectionId) return;
        await api.addToCollection(collectionId, item.id);
        await refresh();
      }
    }),
    [collections, refresh]
  );

  const scan = useCallback(async ({ silent = false, force = false }: { silent?: boolean; force?: boolean } = {}) => {
    if (scanInFlight.current) return;
    if (!force && Date.now() - lastScanAt.current < 10_000) return;
    if (!snapshot?.settings.sourceFolders.length) return;
    scanInFlight.current = true;
    if (!silent) {
      setBusy(true);
      setError(null);
    }
    try {
      await api.scanSources();
      lastScanAt.current = Date.now();
      await refresh();
    } catch (err) {
      if (!silent) setError(String(err));
    } finally {
      scanInFlight.current = false;
      if (!silent) setBusy(false);
    }
  }, [refresh, snapshot?.settings.sourceFolders.length]);

  useEffect(() => {
    if (!snapshot?.settings.sourceFolders.length) return;
    scan({ silent: true, force: true });
  }, [scan, snapshot?.settings.sourceFolders.length]);

  useEffect(() => {
    function maybeScan() {
      if (document.visibilityState === "visible") {
        scan({ silent: true });
      }
    }
    const interval = window.setInterval(maybeScan, 10_000);
    window.addEventListener("focus", maybeScan);
    document.addEventListener("visibilitychange", maybeScan);
    return () => {
      window.clearInterval(interval);
      window.removeEventListener("focus", maybeScan);
      document.removeEventListener("visibilitychange", maybeScan);
    };
  }, [scan]);

  async function chooseInitialSourceFolder() {
    setBusy(true);
    setError(null);
    try {
      const folder = await pickFolder();
      if (!folder || !snapshot) return;
      await api.saveSettings({
        ...snapshot.settings,
        sourceFolders: [folder]
      });
      setSetupDismissed(true);
      await api.scanSources();
      await refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function cleanup(item: MediaItem) {
    if (!window.confirm(`Delete raw file from disk?\n\n${item.filename}`)) return;
    await api.deleteRawFile(item.id);
    await refresh();
  }

  return (
    <div className="app-shell">
      <Sidebar page={page} onPageChange={setPage} />
      <main className="workspace">
        <header className="topbar">
          <div>
            <p className="eyebrow">Reelspace</p>
            <h1>{titleFor(page)}</h1>
          </div>
          <button className="icon-button" onClick={() => scan({ force: true })} disabled={busy} title="Refresh media">
            <RefreshCw size={20} className={busy ? "spin" : undefined} />
          </button>
        </header>
        {error ? <div className="error-banner">{error}</div> : null}
        {page === "home" && snapshot ? (
          <HomePage
            snapshot={snapshot}
            rawItems={rawItems}
            clipItems={clipItems}
            actions={actions}
            onCleanup={cleanup}
          />
        ) : null}
        {page === "gallery" ? <GalleryPage items={rawItems} actions={actions} /> : null}
        {page === "clips" ? <ClipsPage items={clipItems} actions={actions} /> : null}
        {page === "collections" ? (
          <CollectionsPage collections={collections} onCreate={async (name) => {
            await api.createCollection(name);
            await refresh();
          }} />
        ) : null}
        {page === "settings" && snapshot ? (
          <SettingsPage snapshot={snapshot} onSave={async (settings) => {
            setSnapshot(await api.saveSettings(settings));
            await refresh();
          }} />
        ) : null}
      </main>
      {playing ? <PlayerModal item={playing} onClose={() => setPlaying(null)} /> : null}
      {trimming ? (
        <TrimModal
          item={trimming}
          onClose={() => setTrimming(null)}
          onCreated={async () => {
            setTrimming(null);
            await refresh();
            setPage("clips");
          }}
        />
      ) : null}
      {snapshot && !snapshot.settings.sourceFolders.length && !setupDismissed ? (
        <SetupPrompt
          busy={busy}
          onChoose={chooseInitialSourceFolder}
          onDismiss={() => setSetupDismissed(true)}
        />
      ) : null}
    </div>
  );
}

function titleFor(page: PageKey): string {
  return {
    home: "Home",
    gallery: "Gallery",
    clips: "Clips",
    collections: "Collections",
    settings: "Settings"
  }[page];
}
