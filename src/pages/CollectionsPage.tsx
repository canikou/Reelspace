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
  onBack,
  onRemove,
  onStorageDefault,
}: {
  collections: Collection[];
  selected: Collection | null;
  items: MediaItem[];
  actions: MediaActions;
  onCreate: () => void;
  onSelect: (collection: Collection) => void;
  onBack: () => void;
  onRemove: (collection: Collection, item: MediaItem) => void;
  onStorageDefault: (
    collection: Collection,
    policy: "keepLocal" | "archive" | null,
  ) => void;
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
          <div className="collection-policy" aria-label="Collection storage default">
            <span>New items</span>
            <button className={selected.storageDefault === null ? "active" : ""} onClick={() => onStorageDefault(selected, null)}>Inherit</button>
            <button className={selected.storageDefault === "keepLocal" ? "active" : ""} onClick={() => onStorageDefault(selected, "keepLocal")}><AppIcon name="protected" size={14} /> Keep local</button>
            <button className={selected.storageDefault === "archive" ? "active" : ""} onClick={() => onStorageDefault(selected, "archive")}><AppIcon name="archive" size={14} /> Archive</button>
          </div>
        </div>
        <MediaLibrary items={items} actions={{ ...actions, removeFromCollection: (item) => onRemove(selected, item) }} empty="This collection is empty." />
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
                {collection.storageDefault ? <small>{collection.storageDefault === "keepLocal" ? "Keeps new items local" : "Archives new items"}</small> : null}
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
