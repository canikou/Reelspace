import { FolderSearch, Library, Maximize2, NotebookPen, Scissors, Tags, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { mediaSrc } from "../lib/api";
import type { MediaItem } from "../lib/types";
import type { MediaActions } from "./MediaCard";

export function PlayerModal({ item, actions, onClose }: { item: MediaItem; actions: MediaActions; onClose: () => void }) {
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const [error, setError] = useState<string | null>(null);
  const src = mediaSrc(item.path);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onClose();
        return;
      }
      const video = videoRef.current;
      if (!video) return;
      if (event.key === "ArrowLeft") video.currentTime = Math.max(0, video.currentTime - 5);
      if (event.key === "ArrowRight") video.currentTime = Math.min(video.duration || item.durationSeconds, video.currentTime + 5);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [item.durationSeconds, onClose]);

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section className="player-modal" role="dialog" aria-modal="true" aria-labelledby="player-title">
        <header className="modal-header">
          <div>
            <h2 id="player-title">{item.filename}</h2>
            <p>{item.tags.join(", ") || "Untagged"}</p>
          </div>
          <div className="modal-actions">
            {item.kind === "raw" ? <button title="Trim" aria-label="Trim" onClick={() => actions.trim(item)}><Scissors size={18} /></button> : null}
            <button title="Show in Explorer" aria-label="Show in Explorer" onClick={() => actions.reveal(item)}><FolderSearch size={18} /></button>
            <button title="Edit tags" aria-label="Edit tags" onClick={() => { onClose(); actions.tags(item); }}><Tags size={18} /></button>
            <button title="Edit notes" aria-label="Edit notes" onClick={() => { onClose(); actions.notes(item); }}><NotebookPen size={18} /></button>
            <button title="Add to collection" aria-label="Add to collection" onClick={() => { onClose(); actions.addToCollection(item); }}><Library size={18} /></button>
            <button title="Fullscreen" aria-label="Fullscreen" onClick={() => videoRef.current?.requestFullscreen()}><Maximize2 size={18} /></button>
            <button title="Close" aria-label="Close" onClick={onClose}><X size={18} /></button>
          </div>
        </header>
        <div className="player-body">
          {error ? <div className="player-error" role="alert">{error}</div> : null}
          <video
            ref={videoRef}
            src={src}
            controls
            autoPlay
            className="video-player"
            onCanPlay={() => setError(null)}
            onError={() => setError("This file could not be opened by the embedded player. Try View in Explorer if the codec is unsupported.")}
          />
        </div>
      </section>
    </div>
  );
}
