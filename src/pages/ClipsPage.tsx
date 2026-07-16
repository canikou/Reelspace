import { MediaGrid } from "../components/MediaGrid";
import type { MediaActions } from "../components/MediaCard";
import type { MediaItem } from "../lib/types";

export function ClipsPage({ items, actions }: { items: MediaItem[]; actions: MediaActions }) {
  return <MediaGrid items={items} actions={actions} empty="Saved trims appear here as clips." />;
}
