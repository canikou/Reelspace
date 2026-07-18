import { MediaLibrary } from "../components/MediaLibrary";
import type { MediaActions } from "../components/MediaCard";
import { AppIcon } from "../components/icons";
import type { Collection, MediaItem } from "../lib/types";

export function CollectionsPage({
  collections,
  selected,
  items,
  actions,
  onCreate,
  onSelect,
  onBack
}: {
  collections: Collection[];
  selected: Collection | null;
  items: MediaItem[];
  actions: MediaActions;
  onCreate: () => void;
  onSelect: (collection: Collection) => void;
  onBack: () => void;
}) {
  if (selected) {
    return (
      <div className="page-stack">
        <div className="section-title collection-heading">
          <div>
            <button className="bare-icon" onClick={onBack} aria-label="Back to collections" title="Back to collections"><AppIcon name="back" size={18} /></button>
            <h2>{selected.name}</h2>
            <span>{items.length}</span>
          </div>
        </div>
        <MediaLibrary items={items} actions={actions} empty="This collection is empty." />
      </div>
    );
  }

  return (
    <div className="page-stack">
      <div className="section-title">
        <div><h2>Collections</h2><span>{collections.length}</span></div>
        <button onClick={onCreate} title="Create collection"><AppIcon name="add" size={17} /> New</button>
      </div>
      {collections.length ? (
        <section className="collection-grid">
          {collections.map((collection) => (
            <button className="collection-card" key={collection.id} onClick={() => onSelect(collection)}>
              <span className="collection-icon"><AppIcon name="collection" size={21} /></span>
              <span>
                <strong>{collection.name}</strong>
                <small>{collection.itemCount} item{collection.itemCount === 1 ? "" : "s"}</small>
              </span>
            </button>
          ))}
        </section>
      ) : (
        <button className="empty-state empty-action" onClick={onCreate}><AppIcon name="add" size={20} /> Create your first collection</button>
      )}
    </div>
  );
}
