import { Grid2X2, List, Search, SlidersHorizontal, X } from "lucide-react";
import { useMemo, useState } from "react";
import type { MediaItem } from "../lib/types";
import { MediaGrid } from "./MediaGrid";
import type { MediaActions } from "./MediaCard";

type SortKey = "latest" | "oldest" | "largest" | "longest";
type ViewMode = "grid" | "list";

export function MediaLibrary({
  items,
  actions,
  empty
}: {
  items: MediaItem[];
  actions: MediaActions;
  empty: string;
}) {
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<SortKey>("latest");
  const [view, setView] = useState<ViewMode>("grid");
  const visible = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    const filtered = needle
      ? items.filter((item) =>
          [item.filename, item.notes ?? "", ...item.tags].some((value) =>
            value.toLocaleLowerCase().includes(needle)
          )
        )
      : [...items];
    return filtered.sort((a, b) => {
      if (sort === "oldest") return a.modifiedAt - b.modifiedAt;
      if (sort === "largest") return b.fileSize - a.fileSize;
      if (sort === "longest") return b.durationSeconds - a.durationSeconds;
      return b.modifiedAt - a.modifiedAt;
    });
  }, [items, query, sort]);

  return (
    <div className="library-stack">
      <div className="library-toolbar">
        <label className="search-control">
          <Search size={16} />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search media"
            aria-label="Search media"
          />
          {query ? (
            <button onClick={() => setQuery("")} aria-label="Clear search" title="Clear search"><X size={15} /></button>
          ) : null}
        </label>
        <div className="toolbar-meta"><strong>{visible.length}</strong><span>/ {items.length}</span></div>
        <label className="sort-control">
          <SlidersHorizontal size={16} />
          <select value={sort} onChange={(event) => setSort(event.target.value as SortKey)} aria-label="Sort media">
            <option value="latest">Latest</option>
            <option value="oldest">Oldest</option>
            <option value="largest">Largest</option>
            <option value="longest">Longest</option>
          </select>
        </label>
        <div className="view-toggle" aria-label="View density">
          <button className={view === "grid" ? "active" : ""} onClick={() => setView("grid")} aria-label="Grid view" title="Grid view"><Grid2X2 size={16} /></button>
          <button className={view === "list" ? "active" : ""} onClick={() => setView("list")} aria-label="List view" title="List view"><List size={17} /></button>
        </div>
      </div>
      <MediaGrid items={visible} actions={actions} empty={query ? "No media matches this search." : empty} view={view} />
    </div>
  );
}
