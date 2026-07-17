import { Maximize2, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { mediaSrc } from "../lib/api";
import type { MediaItem } from "../lib/types";

export function PlayerModal({ item, onClose }: { item: MediaItem; onClose: () => void }) {
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const [error, setError] = useState<string | null>(null);
  const src = mediaSrc(item.path);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      const video = videoRef.current;
      if (!video) return;
      if (event.key === "ArrowLeft") video.currentTime = Math.max(0, video.currentTime - 5);
      if (event.key === "ArrowRight") video.currentTime = Math.min(video.duration || item.durationSeconds, video.currentTime + 5);
      if (event.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [item.durationSeconds, onClose]);

  return (
    <div className="modal-backdrop">
      <section className="player-modal" role="dialog" aria-modal="true" aria-labelledby="player-title">
        <header className="modal-header">
          <div>
            <h2 id="player-title">{item.filename}</h2>
            <p>{item.tags.join(", ") || "Untagged"}</p>
          </div>
          <div className="modal-actions">
            <button title="Fullscreen" aria-label="Fullscreen" onClick={() => videoRef.current?.requestFullscreen()}><Maximize2 size={18} /></button>
            <button title="Close" aria-label="Close" onClick={onClose}><X size={18} /></button>
          </div>
        </header>
        {error ? <div className="player-error">{error}</div> : null}
        <video
          ref={videoRef}
          src={src}
          controls
          autoPlay
          className="video-player"
          onCanPlay={() => setError(null)}
          onError={() => setError("This file could not be opened by the embedded player. Try View in Explorer if the codec is unsupported.")}
        />
      </section>
    </div>
  );
}
