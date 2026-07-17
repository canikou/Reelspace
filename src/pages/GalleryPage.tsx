import { MediaLibrary } from "../components/MediaLibrary";
import type { MediaActions } from "../components/MediaCard";
import type { MediaItem } from "../lib/types";

export function GalleryPage({ items, actions }: { items: MediaItem[]; actions: MediaActions }) {
  return <MediaLibrary items={items} actions={actions} empty="Scan your source folders to populate the raw gallery." />;
}
