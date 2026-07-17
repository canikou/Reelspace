import { MediaCard, type MediaActions } from "./MediaCard";
import type { MediaItem } from "../lib/types";

export function MediaGrid({
  items,
  actions,
  empty,
  view = "grid"
}: {
  items: MediaItem[];
  actions: MediaActions;
  empty: string;
  view?: "grid" | "list";
}) {
  if (!items.length) {
    return <div className="empty-state">{empty}</div>;
  }
  return (
    <section className={`media-grid ${view}`}>
      {items.map((item) => <MediaCard key={item.id} item={item} actions={actions} view={view} />)}
    </section>
  );
}
