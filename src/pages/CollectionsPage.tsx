import { Plus } from "lucide-react";
import type { Collection } from "../lib/types";

export function CollectionsPage({
  collections,
  onCreate
}: {
  collections: Collection[];
  onCreate: (name: string) => Promise<void>;
}) {
  async function create() {
    const name = window.prompt("Collection name");
    if (!name) return;
    await onCreate(name);
  }

  return (
    <div className="page-stack">
      <div className="section-title">
        <h2>Collections</h2>
        <button onClick={create} title="Create collection"><Plus size={17} /> New</button>
      </div>
      {collections.length ? (
        <section className="collection-grid">
          {collections.map((collection) => (
            <article className="collection-card" key={collection.id}>
              <h3>{collection.name}</h3>
              <p>{collection.itemCount} item{collection.itemCount === 1 ? "" : "s"}</p>
            </article>
          ))}
        </section>
      ) : (
        <div className="empty-state">Collections you create will appear here.</div>
      )}
    </div>
  );
}
