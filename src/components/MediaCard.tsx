import { useEffect, useRef, useState } from "react";
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
  classify?: (item: MediaItem) => void;
  archive?: (item: MediaItem) => void;
  setKeepLocal?: (item: MediaItem, value: boolean) => void;
  openRemote?: (item: MediaItem) => void;
  removeLocal?: (item: MediaItem) => void;
  removeRemote?: (item: MediaItem) => void;
  removeFromCollection?: (item: MediaItem) => void;
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
  const menuRef = useRef<HTMLDetailsElement | null>(null);
  const [menuOpen, setMenuOpen] = useState(false);
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

  useEffect(() => {
    if (!menuOpen) return;
    const close = () => { if (menuRef.current) menuRef.current.open = false; };
    const onPointerDown = (event: PointerEvent) => { if (!menuRef.current?.contains(event.target as Node)) close(); };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      close();
      menuRef.current?.querySelector("summary")?.focus();
    };
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown, true);
    return () => { document.removeEventListener("pointerdown", onPointerDown); document.removeEventListener("keydown", onKeyDown, true); };
  }, [menuOpen]);

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
          {item.gameName ? <span className="game-tag"><AppIcon name="game" size={11} />{item.gameName}</span> : null}
          {item.tags.length ? item.tags.slice(0, 2).map((tag) => <span key={tag}>{tag}</span>) : <span>Untagged</span>}
          {item.tags.length > 2 ? <span className="tag-count">+{item.tags.length - 2}</span> : null}
        </div>
        <h3 title={item.filename}>{item.filename}</h3>
        <p title={detail}>{detail}</p>
      </div>
      <div className="card-actions">
        <button title="Play" aria-label="Play" onClick={() => actions.view(item)}><AppIcon name="play" size={17} /></button>
        {item.kind === "raw" ? <button title="Create clip" aria-label="Create clip" onClick={() => actions.trim(item)}><AppIcon name="clip" size={17} /></button> : null}
        <button title="Show in folder" aria-label="Show in folder" onClick={() => actions.reveal(item)}><AppIcon name="search" size={17} /></button>
        <details ref={menuRef} className="card-menu" onToggle={(event) => setMenuOpen(event.currentTarget.open)}>
          <summary title="More actions" aria-label="More actions"><AppIcon name="more" size={17} /></summary>
          <div onClick={() => { if (menuRef.current) menuRef.current.open = false; }}>
            <button onClick={() => actions.tags(item)}><AppIcon name="tags" size={16} /> Moment tags</button>
            <button onClick={() => actions.notes(item)}><AppIcon name="notes" size={16} /> Notes</button>
            {actions.classify ? <button onClick={() => actions.classify?.(item)}><AppIcon name="game" size={16} /> Classify game</button> : null}
            <button onClick={() => actions.addToCollection(item)}><AppIcon name="collection" size={16} /> Add to collection</button>
            {actions.removeFromCollection ? <button className="destructive-menu-item" onClick={() => actions.removeFromCollection?.(item)}><AppIcon name="cancel" size={16} /> Remove from collection</button> : null}
            {actions.archive && (availability === "localOnly" || availability === "uploadFailed") ? <button onClick={() => actions.archive?.(item)}><AppIcon name="archive" size={16} /> Archive privately</button> : null}
            {actions.setKeepLocal ? <button onClick={() => actions.setKeepLocal?.(item, !item.keepLocal)}><AppIcon name="protected" size={16} /> {item.keepLocal ? "Disable Keep Local" : "Keep Local"}</button> : null}
            {actions.openRemote && item.primaryRemoteUrl ? <button onClick={() => actions.openRemote?.(item)}><AppIcon name="play" size={16} /> Open remote copy</button> : null}
            {actions.removeLocal && availability === "archivedVerified" && !item.keepLocal ? <button className="destructive-menu-item" onClick={() => actions.removeLocal?.(item)}><AppIcon name="cleanup" size={16} /> Remove local copy</button> : null}
            {actions.removeRemote && item.primaryRemoteUrl ? <button className="destructive-menu-item" onClick={() => actions.removeRemote?.(item)}><AppIcon name="error" size={16} /> Delete remote copy</button> : null}
          </div>
        </details>
      </div>
    </article>
  );
}
