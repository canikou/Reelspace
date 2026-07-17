import { ChevronLeft, ChevronRight, Save, Volume2, VolumeX, X } from "lucide-react";
import { type MouseEvent as ReactMouseEvent, useEffect, useMemo, useRef, useState } from "react";
import { api, mediaSrc } from "../lib/api";
import { formatDuration } from "../lib/format";
import type { AudioTrack, MediaItem } from "../lib/types";

type ActivePoint = "start" | "end";
type VideoWithAudioTracks = HTMLVideoElement & {
  audioTracks?: { length: number; [index: number]: { enabled: boolean } };
};

const MIN_SPAN = 0.05;
const NUDGE_SECONDS = 0.1;

export function TrimModal({
  item,
  onClose,
  onCreated
}: {
  item: MediaItem;
  onClose: () => void;
  onCreated: (created: MediaItem) => void | Promise<void>;
}) {
  const videoRef = useRef<VideoWithAudioTracks | null>(null);
  const audioRefs = useRef<Record<number, HTMLAudioElement | null>>({});
  const [duration, setDuration] = useState(Math.max(MIN_SPAN, item.durationSeconds || 1));
  const [start, setStart] = useState(0);
  const [end, setEnd] = useState(Math.max(MIN_SPAN, item.durationSeconds || 1));
  const [activePoint, setActivePoint] = useState<ActivePoint>("start");
  const [playhead, setPlayhead] = useState(0);
  const [tracks, setTracks] = useState<AudioTrack[]>([]);
  const [tracksLoading, setTracksLoading] = useState(true);
  const [enabledTracks, setEnabledTracks] = useState<number[]>([]);
  const [previewSources, setPreviewSources] = useState<Record<number, string>>({});
  const [previewErrors, setPreviewErrors] = useState<Record<number, string>>({});
  const [previewsSettled, setPreviewsSettled] = useState(false);
  const [previewMuted, setPreviewMuted] = useState(false);
  const [previewVolume, setPreviewVolume] = useState(1);
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [trackError, setTrackError] = useState<string | null>(null);
  const [videoError, setVideoError] = useState<string | null>(null);
  const src = mediaSrc(item.path);
  const span = useMemo(() => Math.max(0, end - start), [end, start]);

  useEffect(() => {
    let active = true;
    api.getMediaAudioTracks(item.id)
      .then(async (next) => {
        if (!active) return;
        setTracks(next);
        setEnabledTracks(next.map((track) => track.index));
        await Promise.all(next.map(async (track) => {
          try {
            const path = await api.getAudioTrackPreview(item.id, track.index);
            if (active) {
              setPreviewSources((current) => ({ ...current, [track.index]: mediaSrc(path) ?? "" }));
            }
          } catch (reason) {
            if (active) {
              setPreviewErrors((current) => ({ ...current, [track.index]: String(reason) }));
            }
          }
        }));
        if (active) setPreviewsSettled(true);
      })
      .catch((reason) => active && setTrackError(String(reason)))
      .finally(() => active && setTracksLoading(false));
    return () => { active = false; };
  }, [item.id]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape" && !busy) onClose();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [busy, onClose]);

  useEffect(() => {
    applyAudioPreview(enabledTracks);
  }, [enabledTracks, previewMuted, previewSources, previewVolume, previewsSettled, tracks]);

  const proxiesActive = tracks.length > 0
    && previewsSettled
    && tracks.some((track) => previewSources[track.index]);

  function applyAudioPreview(enabled: number[]) {
    const video = videoRef.current;
    if (!video) return;
    video.muted = enabled.length === 0 || proxiesActive;
    if (proxiesActive) {
      syncAudio(false);
      return;
    }
    const nativeTracks = video.audioTracks;
    if (!nativeTracks) return;
    for (let index = 0; index < nativeTracks.length; index += 1) {
      const stream = tracks[index];
      nativeTracks[index].enabled = Boolean(stream && enabled.includes(stream.index));
    }
  }

  function syncAudio(forceSeek: boolean) {
    const video = videoRef.current;
    if (!video) return;
    for (const track of tracks) {
      const audio = audioRefs.current[track.index];
      if (!audio) continue;
      audio.muted = previewMuted || !enabledTracks.includes(track.index);
      audio.volume = previewVolume;
      audio.playbackRate = video.playbackRate;
      if (forceSeek || Math.abs(audio.currentTime - video.currentTime) > 0.12) {
        audio.currentTime = video.currentTime;
      }
      if (video.paused) audio.pause();
      else void audio.play().catch(() => undefined);
    }
  }

  function seek(value: number) {
    const video = videoRef.current;
    if (!video) return;
    video.pause();
    video.currentTime = value;
    setPlayhead(value);
  }

  function updateStart(value: number) {
    const next = Math.min(Math.max(0, value), end - MIN_SPAN);
    setStart(next);
    setActivePoint("start");
    seek(next);
  }

  function updateEnd(value: number) {
    const next = Math.max(Math.min(duration, value), start + MIN_SPAN);
    setEnd(next);
    setActivePoint("end");
    seek(next);
  }

  function timelineSeconds(event: ReactMouseEvent<HTMLDivElement>) {
    const rect = event.currentTarget.getBoundingClientRect();
    return Math.max(0, Math.min(duration, ((event.clientX - rect.left) / rect.width) * duration));
  }

  function setTimelinePoint(event: ReactMouseEvent<HTMLDivElement>) {
    if (event.button === 0) updateStart(timelineSeconds(event));
    if (event.button === 2) updateEnd(timelineSeconds(event));
  }

  function nudge(direction: -1 | 1, large = false) {
    const amount = (large ? 1 : NUDGE_SECONDS) * direction;
    if (activePoint === "start") updateStart(start + amount);
    else updateEnd(end + amount);
  }

  function toggleTrack(index: number) {
    setEnabledTracks((current) =>
      current.includes(index) ? current.filter((value) => value !== index) : [...current, index]
    );
  }

  async function saveClip() {
    setBusy(true);
    setError(null);
    try {
      const created = await api.createClip({
        sourceId: item.id,
        startSeconds: start,
        endSeconds: end,
        title: title || null,
        enabledAudioTrackIndices: enabledTracks
      });
      await onCreated(created);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop">
      <section className="trim-modal" role="dialog" aria-modal="true" aria-labelledby="trim-title">
        <header className="modal-header">
          <div>
            <h2 id="trim-title">Trim</h2>
            <p>{item.filename}</p>
          </div>
          <button title="Close" aria-label="Close" onClick={onClose} disabled={busy}><X size={18} /></button>
        </header>
        <video
          ref={videoRef}
          src={src}
          controls
          className="video-player trim-video"
          onLoadedMetadata={(event) => {
            const measured = event.currentTarget.duration;
            const nextDuration = Number.isFinite(measured) && measured > MIN_SPAN ? measured : duration;
            setDuration(nextDuration);
            setStart((current) => Math.min(Math.max(0, current), nextDuration - MIN_SPAN));
            setEnd((current) => Math.max(MIN_SPAN, Math.min(current, nextDuration)));
            applyAudioPreview(enabledTracks);
          }}
          onPlay={() => syncAudio(true)}
          onPause={() => syncAudio(false)}
          onSeeking={() => syncAudio(true)}
          onRateChange={() => syncAudio(false)}
          onVolumeChange={(event) => {
            if (!proxiesActive) return;
            setPreviewVolume(event.currentTarget.volume);
            event.currentTarget.muted = true;
            syncAudio(false);
          }}
          onTimeUpdate={(event) => { setPlayhead(event.currentTarget.currentTime); syncAudio(false); }}
          onCanPlay={() => setVideoError(null)}
          onError={() => setVideoError("This file could not be opened by the embedded player. FFmpeg may still be able to trim it.")}
        />
        <div className="audio-proxies" aria-hidden="true">
          {tracks.map((track) => previewSources[track.index] ? (
            <audio
              key={track.index}
              ref={(element) => { audioRefs.current[track.index] = element; }}
              src={previewSources[track.index]}
              preload="auto"
            />
          ) : null)}
        </div>
        <div className="trim-panel">
          <label className="trim-title-field">
            Clip title
            <input value={title} onChange={(event) => setTitle(event.target.value)} placeholder="Optional" />
          </label>

          <div className="range-readout">
            <button className={activePoint === "start" ? "active start" : "start"} onClick={() => { setActivePoint("start"); seek(start); }}>
              <span>IN</span><strong>{formatDuration(start)}</strong>
            </button>
            <div><span>SELECTED</span><strong>{formatDuration(span)}</strong></div>
            <button className={activePoint === "end" ? "active end" : "end"} onClick={() => { setActivePoint("end"); seek(end); }}>
              <span>OUT</span><strong>{formatDuration(end)}</strong>
            </button>
          </div>

          <div
            className="trim-timeline"
            onMouseDown={setTimelinePoint}
            onContextMenu={(event) => event.preventDefault()}
            title="Left click sets IN. Right click sets OUT."
          >
            <div className="trim-selection" style={{ left: `${(start / duration) * 100}%`, width: `${(span / duration) * 100}%` }} />
            <div className="trim-playhead" style={{ left: `${(playhead / duration) * 100}%` }} />
            <button
              className={`trim-handle start${activePoint === "start" ? " active" : ""}`}
              style={{ left: `${(start / duration) * 100}%` }}
              onMouseDown={(event) => event.stopPropagation()}
              onClick={() => { setActivePoint("start"); seek(start); }}
              onFocus={() => setActivePoint("start")}
              onKeyDown={(event) => {
                if (event.key === "ArrowLeft") { event.preventDefault(); updateStart(start - (event.shiftKey ? 1 : NUDGE_SECONDS)); }
                if (event.key === "ArrowRight") { event.preventDefault(); updateStart(start + (event.shiftKey ? 1 : NUDGE_SECONDS)); }
              }}
              role="slider"
              aria-valuemin={0}
              aria-valuemax={Math.max(0, end - MIN_SPAN)}
              aria-valuenow={start}
              aria-valuetext={formatDuration(start)}
              aria-label="Trim start"
            />
            <button
              className={`trim-handle end${activePoint === "end" ? " active" : ""}`}
              style={{ left: `${(end / duration) * 100}%` }}
              onMouseDown={(event) => event.stopPropagation()}
              onClick={() => { setActivePoint("end"); seek(end); }}
              onFocus={() => setActivePoint("end")}
              onKeyDown={(event) => {
                if (event.key === "ArrowLeft") { event.preventDefault(); updateEnd(end - (event.shiftKey ? 1 : NUDGE_SECONDS)); }
                if (event.key === "ArrowRight") { event.preventDefault(); updateEnd(end + (event.shiftKey ? 1 : NUDGE_SECONDS)); }
              }}
              role="slider"
              aria-valuemin={Math.min(duration, start + MIN_SPAN)}
              aria-valuemax={duration}
              aria-valuenow={end}
              aria-valuetext={formatDuration(end)}
              aria-label="Trim end"
            />
          </div>

          <div className="nudge-controls">
            <button onClick={(event) => nudge(-1, event.shiftKey)} title="Nudge left; hold Shift for 1 second" aria-label={`Nudge ${activePoint} left`}><ChevronLeft size={17} /></button>
            <span>{activePoint === "start" ? "IN" : "OUT"} · {formatDuration(activePoint === "start" ? start : end)}</span>
            <button onClick={(event) => nudge(1, event.shiftKey)} title="Nudge right; hold Shift for 1 second" aria-label={`Nudge ${activePoint} right`}><ChevronRight size={17} /></button>
          </div>

          {tracks.length ? (
            <section className="track-controls" aria-label="Audio tracks">
              <div className="track-heading">
                <div><Volume2 size={15} /><span>Audio tracks</span></div>
                {proxiesActive ? (
                  <div className="proxy-volume">
                    <button onClick={() => setPreviewMuted((current) => !current)} aria-label={previewMuted ? "Unmute track preview" : "Mute track preview"} aria-pressed={previewMuted}>
                      {previewMuted ? <VolumeX size={14} /> : <Volume2 size={14} />}
                    </button>
                    <input aria-label="Track preview volume" type="range" min="0" max="1" step="0.05" value={previewVolume} onChange={(event) => setPreviewVolume(Number(event.target.value))} />
                  </div>
                ) : null}
              </div>
              <div className="track-list" aria-live="polite">
                {tracks.map((track, position) => {
                  const enabled = enabledTracks.includes(track.index);
                  return (
                    <button key={track.index} className={enabled ? "enabled" : ""} onClick={() => toggleTrack(track.index)} aria-pressed={enabled}>
                      {enabled ? <Volume2 size={15} /> : <VolumeX size={15} />}
                      <span>{track.title || `Track ${position + 1}`}</span>
                      <small>{previewSources[track.index]
                        ? [track.codec?.toUpperCase(), track.channels ? `${track.channels}ch` : null].filter(Boolean).join(" · ")
                        : previewErrors[track.index] ? "Preview unavailable" : "Preparing…"}</small>
                    </button>
                  );
                })}
              </div>
            </section>
          ) : null}

          {videoError ? <div className="inline-error" role="alert">{videoError}</div> : null}
          {trackError ? <div className="inline-error" role="alert">Audio tracks unavailable: {trackError}</div> : null}
          {Object.keys(previewErrors).length ? (
            <div className="inline-error" role="alert">Some tracks could not be previewed. Their selection will still be applied to the saved clip.</div>
          ) : null}
          {error ? <div className="inline-error" role="alert">{error}</div> : null}
          <div className="trim-actions">
            <button className="primary-button" onClick={saveClip} disabled={busy || tracksLoading || Boolean(trackError) || span < MIN_SPAN} title="Save clip">
              <Save size={17} /> {busy ? "Saving" : tracksLoading ? "Loading tracks" : "Save Clip"}
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}
