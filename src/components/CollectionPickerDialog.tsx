import { Library, Plus } from "lucide-react";
import { useState } from "react";
import type { Collection, MediaItem } from "../lib/types";
import { Dialog } from "./Dialog";

export function CollectionPickerDialog({
  item,
  collections,
  onClose,
  onPick,
  onCreate
}: {
  item: MediaItem;
  collections: Collection[];
  onClose: () => void;
  onPick: (collection: Collection) => Promise<void>;
  onCreate: () => void;
}) {
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function pick(collection: Collection) {
    setBusyId(collection.id);
    setError(null);
    try {
      await onPick(collection);
    } catch (reason) {
      setError(String(reason));
      setBusyId(null);
    }
  }

  return (
    <Dialog title="Add to collection" description={item.filename} onClose={onClose}>
      <div className="collection-picker">
        {collections.map((collection) => (
          <button key={collection.id} onClick={() => pick(collection)} disabled={busyId !== null}>
            <Library size={17} />
            <span>{collection.name}</span>
            <small>{busyId === collection.id ? "…" : collection.itemCount}</small>
          </button>
        ))}
        <button className="create-row" onClick={onCreate}>
          <Plus size={17} /> <span>New collection</span>
        </button>
        {error ? <div className="inline-error">{error}</div> : null}
      </div>
    </Dialog>
  );
}
