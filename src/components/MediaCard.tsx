import { useEffect, useRef } from "react";
import { mediaSrc } from "../lib/api";
import { formatBytes, formatDuration, formatShortDate } from "../lib/format";
import type { MediaItem } from "../lib/types";
import { AppIcon, AvailabilityIcon, type AvailabilityState } from "./icons";

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
  view = "grid",
  highlighted = false
}: {
  item: MediaItem;
  actions: MediaActions;
  view?: "grid" | "list";
  highlighted?: boolean;
}) {
  const cardRef = useRef<HTMLElement | null>(null);
  const thumbnailSrc = mediaSrc(item.thumbnailPath);
  const availability = (item.availability ?? "localOnly") as AvailabilityState;
  const detail = [
    item.width && item.height ? `${item.width}×${item.height}` : null,
    item.codec?.toUpperCase(),
    formatBytes(item.fileSize),
    formatShortDate(item.modifiedAt)
  ].filter(Boolean).join(" · ");

  useEffect(() => {
    if (highlighted) cardRef.current?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [highlighted]);

  return (
    <article ref={cardRef} className={`media-card ${view}${highlighted ? " highlighted" : ""}`}>
      <button className="preview-button" onClick={() => actions.view(item)} title="Play" aria-label={`Play ${item.filename}`}>
        {thumbnailSrc ? (
          <img src={thumbnailSrc} alt="" loading="lazy" decoding="async" />
        ) : (
          <div className="empty-preview"><AppIcon name={item.kind === "clip" ? "clip" : "recording"} size={24} /><span>No preview</span></div>
        )}
        <span className={`kind-pill ${item.kind}`}>{item.kind}</span>
        <AvailabilityIcon state={availability} className="card-availability" />
        {item.keepLocal ? <span className="keep-local-badge" title="Protected from automatic cleanup" aria-label="Keep Local enabled"><AppIcon name="protected" size={13} /></span> : null}
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
        <button title="Play" aria-label="Play" onClick={() => actions.view(item)}><AppIcon name="play" size={17} /></button>
        {item.kind === "raw" ? <button title="Create clip" aria-label="Create clip" onClick={() => actions.trim(item)}><AppIcon name="clip" size={17} /></button> : null}
        <button title="Show in Explorer" aria-label="Show in Explorer" onClick={() => actions.reveal(item)}><AppIcon name="search" size={17} /></button>
        <details className="card-menu">
          <summary title="More actions" aria-label="More actions"><AppIcon name="more" size={17} /></summary>
          <div>
            <button onClick={() => actions.tags(item)}><AppIcon name="tags" size={16} /> Moment tags</button>
            <button onClick={() => actions.notes(item)}><AppIcon name="notes" size={16} /> Notes</button>
            <button onClick={() => actions.addToCollection(item)}><AppIcon name="collection" size={16} /> Add to collection</button>
          </div>
        </details>
      </div>
    </article>
  );
}
