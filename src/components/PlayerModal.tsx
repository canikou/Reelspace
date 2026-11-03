import { useEffect, useRef, useState } from "react";
import { mediaSrc } from "../lib/api";
import type { MediaItem } from "../lib/types";
import type { MediaActions } from "./MediaCard";
import { AppIcon } from "./icons";

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
      // The focused <video> and buttons handle their own keys; only act from the dialog background.
      if (!video || event.target === video || (event.target as HTMLElement).closest?.("button, input, select, textarea")) return;
      if (event.key === " ") { event.preventDefault(); if (video.paused) void video.play(); else video.pause(); }
      if (event.key === "ArrowLeft") video.currentTime = Math.max(0, video.currentTime - 5);
      if (event.key === "ArrowRight") video.currentTime = Math.min(video.duration || item.durationSeconds, video.currentTime + 5);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [item.durationSeconds, onClose]);

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    videoRef.current?.focus();
    return () => previous?.focus();
  }, []);

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section className="player-modal" role="dialog" aria-modal="true" aria-labelledby="player-title">
        <header className="modal-header">
          <div>
            <h2 id="player-title">{item.filename}</h2>
            <p>{item.tags.join(", ") || "Untagged"}</p>
          </div>
          <div className="modal-actions">
            {item.kind === "raw" ? <button title="Create clip" aria-label="Create clip" onClick={() => actions.trim(item)}><AppIcon name="clip" size={18} /></button> : null}
            <button title="Show in folder" aria-label="Show in folder" onClick={() => actions.reveal(item)}><AppIcon name="search" size={18} /></button>
            <button title="Edit moment tags" aria-label="Edit moment tags" onClick={() => { onClose(); actions.tags(item); }}><AppIcon name="tags" size={18} /></button>
            <button title="Edit notes" aria-label="Edit notes" onClick={() => { onClose(); actions.notes(item); }}><AppIcon name="notes" size={18} /></button>
            <button title="Add to collection" aria-label="Add to collection" onClick={() => { onClose(); actions.addToCollection(item); }}><AppIcon name="collection" size={18} /></button>
            <button title="Fullscreen" aria-label="Fullscreen" onClick={() => videoRef.current?.requestFullscreen()}><AppIcon name="maximize" size={18} /></button>
            <button title="Close" aria-label="Close" onClick={onClose}><AppIcon name="close" size={18} /></button>
          </div>
        </header>
        <div className="player-body">
          {error ? <div className="player-error" role="alert">{error}</div> : null}
          <video
            ref={videoRef}
            tabIndex={0}
            src={src}
            controls
            autoPlay
            className="video-player"
            onCanPlay={() => setError(null)}
            onError={() => setError("This file could not be opened by the embedded player. Try Show in folder if the codec is unsupported.")}
          />
        </div>
      </section>
    </div>
  );
}
