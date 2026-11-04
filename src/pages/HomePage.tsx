import { MediaRail } from "../components/MediaRail";
import { StatGrid } from "../components/StatGrid";
import type { MediaActions } from "../components/MediaCard";
import { AppIcon } from "../components/icons";
import { formatBytes } from "../lib/format";
import type { AppSnapshot, MediaItem } from "../lib/types";

export function HomePage({
  snapshot,
  rawItems,
  clipItems,
  actions,
  onCleanup
}: {
  snapshot: AppSnapshot;
  rawItems: MediaItem[];
  clipItems: MediaItem[];
  actions: MediaActions;
  onCleanup: (item: MediaItem) => void;
}) {
  const oldestRaw = [...rawItems].sort((a, b) => a.createdAt - b.createdAt).slice(0, 5);
  return (
    <div className="page-stack">
      <StatGrid stats={snapshot.stats} rawCount={rawItems.length} clipCount={clipItems.length} />
      {snapshot.stats.isNearLimit ? (
        <section className="cleanup-band">
          <AppIcon name="warning" size={22} />
          <div>
            <h2>Storage limit is close</h2>
            <p>Clips are protected. Cleanup only removes raw files you confirm.</p>
          </div>
          {oldestRaw[0] ? (
            <button onClick={() => onCleanup(oldestRaw[0])} title="Delete the oldest raw file">
              Review {formatBytes(oldestRaw[0].fileSize)}
            </button>
          ) : null}
        </section>
      ) : null}
      <MediaRail title="Latest recordings" items={rawItems.slice(0, 12)} actions={actions} empty="No recordings yet." />
      <MediaRail title="Latest clips" items={clipItems.slice(0, 12)} actions={actions} empty="No clips yet. Open a recording and choose Create clip." />
    </div>
  );
}
