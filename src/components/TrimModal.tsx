import { Save, X } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { api, mediaSrc } from "../lib/api";
import { formatDuration } from "../lib/format";
import type { MediaItem } from "../lib/types";

export function TrimModal({
  item,
  onClose,
  onCreated
}: {
  item: MediaItem;
  onClose: () => void;
  onCreated: () => void;
}) {
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const [start, setStart] = useState(0);
  const [end, setEnd] = useState(Math.max(1, Math.floor(item.durationSeconds || 1)));
  const [accurate, setAccurate] = useState(false);
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const src = mediaSrc(item.path);

  const duration = Math.max(1, Math.floor(item.durationSeconds || 1));
  const span = useMemo(() => Math.max(0, end - start), [end, start]);

  function updateStart(value: number) {
    const next = Math.min(Math.max(0, value), end - 1);
    setStart(next);
    if (videoRef.current) videoRef.current.currentTime = next;
  }

  function updateEnd(value: number) {
    const next = Math.max(Math.min(duration, value), start + 1);
    setEnd(next);
    if (videoRef.current) videoRef.current.currentTime = next;
  }

  async function saveClip() {
    setBusy(true);
    setError(null);
    try {
      await api.createClip({
        sourceId: item.id,
        startSeconds: start,
        endSeconds: end,
        title: title || null,
        accurate
      });
      await onCreated();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop">
      <section className="trim-modal">
        <header className="modal-header">
          <div>
            <h2>Trim</h2>
            <p>{item.filename}</p>
          </div>
          <button title="Close" onClick={onClose}><X size={18} /></button>
        </header>
        <video
          ref={videoRef}
          src={src}
          controls
          className="video-player trim-video"
          onCanPlay={() => setError(null)}
          onError={() => setError("This file could not be opened by the embedded player. The saved trim can still work if FFmpeg supports it.")}
        />
        <div className="trim-panel">
          <label>
            Clip title
            <input value={title} onChange={(event) => setTitle(event.target.value)} placeholder="Optional" />
          </label>
          <label className="toggle-row">
            <input type="checkbox" checked={accurate} onChange={(event) => setAccurate(event.target.checked)} />
            Accurate NVENC trim
          </label>
          <div className="range-readout">
            <span>{formatDuration(start)}</span>
            <strong>{formatDuration(span)}</strong>
            <span>{formatDuration(end)}</span>
          </div>
          <div className="range-stack">
            <input
              type="range"
              min={0}
              max={duration}
              step={0.1}
              value={start}
              onChange={(event) => updateStart(Number(event.target.value))}
              title="Trim start"
            />
            <input
              type="range"
              min={0}
              max={duration}
              step={0.1}
              value={end}
              onChange={(event) => updateEnd(Number(event.target.value))}
              title="Trim end"
            />
          </div>
          {error ? <div className="error-banner">{error}</div> : null}
          <button className="primary-button" onClick={saveClip} disabled={busy || span <= 0} title="Save clip">
            <Save size={17} />
            {busy ? "Saving..." : "Save Clip"}
          </button>
        </div>
      </section>
    </div>
  );
}
