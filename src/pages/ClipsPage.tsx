import { MediaLibrary } from "../components/MediaLibrary";
import type { MediaActions } from "../components/MediaCard";
import type { MediaItem } from "../lib/types";

export function ClipsPage({ items, actions, highlightedId }: { items: MediaItem[]; actions: MediaActions; highlightedId?: string | null }) {
  return <MediaLibrary items={items} actions={actions} highlightedId={highlightedId} empty="Saved trims appear here as clips." />;
}
