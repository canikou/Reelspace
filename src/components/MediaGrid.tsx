import { MediaCard, type MediaActions } from "./MediaCard";
import type { MediaItem } from "../lib/types";

export function MediaGrid({ items, actions, empty }: { items: MediaItem[]; actions: MediaActions; empty: string }) {
  if (!items.length) {
    return <div className="empty-state">{empty}</div>;
  }
  return (
    <section className="media-grid">
      {items.map((item) => <MediaCard key={item.id} item={item} actions={actions} />)}
    </section>
  );
}
