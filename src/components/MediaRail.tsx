import { MediaCard, type MediaActions } from "./MediaCard";
import type { MediaItem } from "../lib/types";

export function MediaRail({ title, items, actions }: { title: string; items: MediaItem[]; actions: MediaActions }) {
  return (
    <section className="rail-section">
      <div className="section-title">
        <h2>{title}</h2>
        <span>{items.length}</span>
      </div>
      {items.length ? (
        <div className="media-rail">
          {items.slice(0, 12).map((item) => <MediaCard key={item.id} item={item} actions={actions} />)}
        </div>
      ) : (
        <div className="empty-state compact">Nothing here yet.</div>
      )}
    </section>
  );
}
