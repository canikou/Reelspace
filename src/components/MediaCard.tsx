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

export function MediaCard({ item, actions }: { item: MediaItem; actions: MediaActions }) {
  const thumbnailSrc = mediaSrc(item.thumbnailPath);
  const videoSrc = mediaSrc(item.path);

  return (
    <article className="media-card">
      <button className="preview-button" onClick={() => actions.view(item)} title="View">
        {thumbnailSrc ? (
          <img src={thumbnailSrc} alt="" />
        ) : videoSrc ? (
          <video className="card-video-preview" src={videoSrc} muted preload="metadata" />
        ) : (
          <div className="empty-preview">No preview</div>
        )}
        <span className="duration-pill">{formatDuration(item.durationSeconds)}</span>
      </button>
      <div className="card-body">
        <div className="tag-row">
          {item.tags.length ? item.tags.slice(0, 3).map((tag) => <span key={tag}>{tag}</span>) : <span>Untagged</span>}
        </div>
        <h3 title={item.filename}>{item.filename}</h3>
        <p>{formatBytes(item.fileSize)} - {formatShortDate(item.modifiedAt)}</p>
      </div>
      <div className="card-actions">
        <button title="View" onClick={() => actions.view(item)}><Eye size={17} /></button>
        {item.kind === "raw" ? <button title="Trim" onClick={() => actions.trim(item)}><Scissors size={17} /></button> : null}
        <button title="View in Explorer" onClick={() => actions.reveal(item)}><FolderSearch size={17} /></button>
        <button title="Edit tags" onClick={() => actions.tags(item)}><Tags size={17} /></button>
        <button title="Add notes" onClick={() => actions.notes(item)}><NotebookPen size={17} /></button>
        <button title="Add to collection" onClick={() => actions.addToCollection(item)}><Library size={17} /></button>
        <button title="More"><MoreVertical size={17} /></button>
      </div>
    </article>
  );
}
