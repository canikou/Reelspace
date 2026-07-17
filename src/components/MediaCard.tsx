import { Eye, FolderSearch, MoreVertical, NotebookPen, Scissors, Tags, Library } from "lucide-react";
import { mediaSrc } from "../lib/api";
import { formatBytes, formatDuration, formatShortDate } from "../lib/format";
import type { MediaItem } from "../lib/types";

export interface MediaActions {
  view: (item: MediaItem) => void;
  trim: (item: MediaItem) => void;
  reveal: (item: MediaItem) => void;
  notes: (item: MediaItem) => void;
  tags: (item: MediaItem) => void;
  addToCollection: (item: MediaItem) => void;
}

export function MediaCard({
  item,
  actions,
  view = "grid"
}: {
  item: MediaItem;
  actions: MediaActions;
  view?: "grid" | "list";
}) {
  const thumbnailSrc = mediaSrc(item.thumbnailPath);
  const detail = [
    item.width && item.height ? `${item.width}×${item.height}` : null,
    item.codec?.toUpperCase(),
    formatBytes(item.fileSize),
    formatShortDate(item.modifiedAt)
  ].filter(Boolean).join(" · ");

  return (
    <article className={`media-card ${view}`}>
      <button className="preview-button" onClick={() => actions.view(item)} title="Play" aria-label={`Play ${item.filename}`}>
        {thumbnailSrc ? (
          <img src={thumbnailSrc} alt="" loading="lazy" decoding="async" />
        ) : (
          <div className="empty-preview"><Eye size={22} /><span>No preview</span></div>
        )}
        <span className={`kind-pill ${item.kind}`}>{item.kind}</span>
        <span className="duration-pill">{formatDuration(item.durationSeconds)}</span>
      </button>
      <div className="card-body">
        <div className="tag-row">
          {item.tags.length ? item.tags.slice(0, 2).map((tag) => <span key={tag}>{tag}</span>) : <span>Untagged</span>}
          {item.tags.length > 2 ? <span className="tag-count">+{item.tags.length - 2}</span> : null}
        </div>
        <h3 title={item.filename}>{item.filename}</h3>
        <p title={detail}>{detail}</p>
      </div>
      <div className="card-actions">
        <button title="Play" aria-label="Play" onClick={() => actions.view(item)}><Eye size={17} /></button>
        {item.kind === "raw" ? <button title="Trim" aria-label="Trim" onClick={() => actions.trim(item)}><Scissors size={17} /></button> : null}
        <button title="Show in Explorer" aria-label="Show in Explorer" onClick={() => actions.reveal(item)}><FolderSearch size={17} /></button>
        <details className="card-menu">
          <summary title="More actions" aria-label="More actions"><MoreVertical size={17} /></summary>
          <div>
            <button onClick={() => actions.tags(item)}><Tags size={16} /> Tags</button>
            <button onClick={() => actions.notes(item)}><NotebookPen size={16} /> Notes</button>
            <button onClick={() => actions.addToCollection(item)}><Library size={16} /> Collection</button>
          </div>
        </details>
      </div>
    </article>
  );
}
