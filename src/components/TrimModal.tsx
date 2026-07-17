import { Check, ChevronLeft, ChevronRight, LoaderCircle, Save, Volume2, VolumeX, X } from "lucide-react";
import { type MouseEvent as ReactMouseEvent, useEffect, useMemo, useRef, useState } from "react";
import { api, mediaSrc } from "../lib/api";
import { formatDuration } from "../lib/format";
import type { AudioTrack, MediaItem } from "../lib/types";

type ActivePoint = "start" | "end";
type PreviewPhase = "queued" | "preparing" | "ready" | "error";
type PreviewStatus = { phase: PreviewPhase; method?: "cache" | "copy" | "transcode"; elapsedMs?: number };
type VideoWithAudioTracks = HTMLVideoElement & {
  audioTracks?: { length: number; [index: number]: { enabled: boolean } };
};

const MIN_SPAN = 0.05;
const NUDGE_SECONDS = 0.1;

function createTrimSessionId() {
  return globalThis.crypto?.randomUUID?.() ?? `trim-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

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
  const mountedRef = useRef(true);
  const generationRef = useRef(0);
  const openedAtRef = useRef(performance.now());
  const metadataLoggedRef = useRef(false);
  const canPlayLoggedRef = useRef(false);
  const sessionIdRef = useRef(createTrimSessionId());
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
  const [previewStatus, setPreviewStatus] = useState<Record<number, PreviewStatus>>({});
  const [previewMuted, setPreviewMuted] = useState(false);
  const [previewVolume, setPreviewVolume] = useState(1);
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [trackError, setTrackError] = useState<string | null>(null);
  const [videoError, setVideoError] = useState<string | null>(null);
  const src = mediaSrc(item.path);
  const span = useMemo(() => Math.max(0, end - start), [end, start]);
  function logTrimEvent(event: string, fields: Record<string, unknown> = {}) {
    void api.logClientEvent(event, {
      sessionId: sessionIdRef.current,
      mediaId: item.id,
      ...fields
    }).catch(() => undefined);
  }

  useEffect(() => {
    const sessionId = createTrimSessionId();
    sessionIdRef.current = sessionId;
    const generation = generationRef.current + 1;
    generationRef.current = generation;
    mountedRef.current = true;
    let active = true;
    const probeStarted = performance.now();
    openedAtRef.current = probeStarted;
    metadataLoggedRef.current = false;
    canPlayLoggedRef.current = false;
    setTracks([]);
    setTracksLoading(true);
    setEnabledTracks([]);
    setPreviewSources({});
    setPreviewErrors({});
    setPreviewStatus({});
    setTrackError(null);
    logTrimEvent("trim.open", {
      sourceBytes: item.fileSize,
      sourceDurationSeconds: item.durationSeconds
    });
    api.getMediaAudioTracks(item.id, sessionId)
      .then((next) => {
        if (!active || generationRef.current !== generation) return;
        setTracks(next);
        setEnabledTracks(next.map((track) => track.index));
        setTracksLoading(false);
        setPreviewStatus(Object.fromEntries(next.map((track) => [track.index, { phase: "queued" as const }])));
        logTrimEvent("audio.probe.ready", {
          trackCount: next.length,
          elapsedMs: Math.round(performance.now() - probeStarted)
        });
        const previewJobs = next.map(async (track) => {
          if (!active || generationRef.current !== generation) return;
          setPreviewStatus((current) => ({ ...current, [track.index]: { phase: "preparing" } }));
          const previewStarted = performance.now();
          logTrimEvent("audio.preview.request", {
            streamIndex: track.index,
            codec: track.codec,
            channels: track.channels
          });
          try {
            const result = await api.getAudioTrackPreview(item.id, track.index, sessionId);
            if (active && generationRef.current === generation) {
              setPreviewSources((current) => ({ ...current, [track.index]: mediaSrc(result.path) ?? "" }));
              setPreviewStatus((current) => ({
                ...current,
                [track.index]: { phase: "ready", method: result.method, elapsedMs: result.elapsedMs }
              }));
              logTrimEvent("audio.preview.ready", {
                streamIndex: track.index,
                method: result.method,
                cacheHit: result.cacheHit,
                backendElapsedMs: result.elapsedMs,
                clientElapsedMs: Math.round(performance.now() - previewStarted),
                outputBytes: result.bytes
              });
            }
          } catch (reason) {
            if (active && generationRef.current === generation && !String(reason).toLowerCase().includes("cancelled")) {
              setPreviewErrors((current) => ({ ...current, [track.index]: String(reason) }));
              setPreviewStatus((current) => ({ ...current, [track.index]: { phase: "error" } }));
              logTrimEvent("audio.preview.error", {
                streamIndex: track.index,
                elapsedMs: Math.round(performance.now() - previewStarted),
                error: String(reason)
              });
            }
          }
        });
        void Promise.allSettled(previewJobs).then(() => {
          if (!active || generationRef.current !== generation) return;
          logTrimEvent("audio.preview.settled", {
            trackCount: next.length,
            elapsedMs: Math.round(performance.now() - probeStarted)
          });
        });
      })
      .catch((reason) => {
        if (!active || generationRef.current !== generation) return;
        setTrackError(String(reason));
        setTracksLoading(false);
        logTrimEvent("audio.probe.error", {
          elapsedMs: Math.round(performance.now() - probeStarted),
          error: String(reason)
        });
      });
    return () => {
      active = false;
      mountedRef.current = false;
      generationRef.current += 1;
      for (const audio of Object.values(audioRefs.current)) audio?.pause();
      void api.cancelAudioPreviewSession(sessionId).catch(() => undefined);
      logTrimEvent("trim.close", {
        elapsedMs: Math.round(performance.now() - openedAtRef.current)
      });
    };
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
  }, [enabledTracks, previewMuted, previewSources, previewVolume, tracks]);

  const proxiesActive = enabledTracks.length > 0
    && enabledTracks.every((index) => Boolean(previewSources[index]));
  const readyPreviewCount = tracks.filter((track) => previewStatus[track.index]?.phase === "ready").length;

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
    if (!proxiesActive) {
      for (const audio of Object.values(audioRefs.current)) audio?.pause();
      return;
    }
    for (const track of tracks) {
      const audio = audioRefs.current[track.index];
      if (!audio) continue;
      const enabled = enabledTracks.includes(track.index);
      audio.muted = previewMuted || !enabled;
      if (!enabled) {
        audio.pause();
        continue;
      }
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
    const saveStarted = performance.now();
    logTrimEvent("trim.save.started", {
      startSeconds: start,
      endSeconds: end,
      selectedDurationSeconds: span,
      audioTrackCount: enabledTracks.length
    });
    try {
      const created = await api.createClip({
        sourceId: item.id,
        sessionId: sessionIdRef.current,
        startSeconds: start,
        endSeconds: end,
        title: title || null,
        enabledAudioTrackIndices: enabledTracks
      });
      logTrimEvent("trim.save.finished", {
        elapsedMs: Math.round(performance.now() - saveStarted),
        outputBytes: created.fileSize,
        outputDurationSeconds: created.durationSeconds
      });
      await onCreated(created);
    } catch (reason) {
      if (mountedRef.current) setError(String(reason));
      logTrimEvent("trim.save.error", {
        elapsedMs: Math.round(performance.now() - saveStarted),
        error: String(reason)
      });
    } finally {
      if (mountedRef.current) setBusy(false);
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
            if (!metadataLoggedRef.current) {
              metadataLoggedRef.current = true;
              logTrimEvent("trim.video.metadata_ready", {
                elapsedMs: Math.round(performance.now() - openedAtRef.current),
                measuredDurationSeconds: nextDuration
              });
            }
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
          onCanPlay={() => {
            setVideoError(null);
            if (!canPlayLoggedRef.current) {
              canPlayLoggedRef.current = true;
              logTrimEvent("trim.video.can_play", {
                elapsedMs: Math.round(performance.now() - openedAtRef.current)
              });
            }
          }}
          onError={() => setVideoError("This file could not be opened by the embedded player. FFmpeg may still be able to trim it.")}
        />
        <div className="audio-proxies" aria-hidden="true">
          {tracks.map((track) => previewSources[track.index] ? (
            <audio
              key={track.index}
              ref={(element) => { audioRefs.current[track.index] = element; }}
              src={previewSources[track.index]}
              preload="metadata"
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
                <div className="track-heading-actions">
                  <span className={readyPreviewCount === tracks.length ? "track-progress ready" : "track-progress"} aria-live="polite">
                    {readyPreviewCount === tracks.length ? <Check size={13} /> : <LoaderCircle size={13} className="spin" />}
                    {readyPreviewCount}/{tracks.length}
                  </span>
                  {proxiesActive ? (
                    <div className="proxy-volume">
                      <button onClick={() => setPreviewMuted((current) => !current)} aria-label={previewMuted ? "Unmute track preview" : "Mute track preview"} aria-pressed={previewMuted}>
                        {previewMuted ? <VolumeX size={14} /> : <Volume2 size={14} />}
                      </button>
                      <input aria-label="Track preview volume" type="range" min="0" max="1" step="0.05" value={previewVolume} onChange={(event) => setPreviewVolume(Number(event.target.value))} />
                    </div>
                  ) : null}
                </div>
              </div>
              <div className="track-progress-rail" aria-hidden="true">
                <span style={{ width: `${tracks.length ? (readyPreviewCount / tracks.length) * 100 : 0}%` }} />
              </div>
              <div className="track-list">
                {tracks.map((track, position) => {
                  const enabled = enabledTracks.includes(track.index);
                  const status = previewStatus[track.index]?.phase ?? "queued";
                  const method = previewStatus[track.index]?.method;
                  const details = [track.codec?.toUpperCase(), track.channels ? `${track.channels}ch` : null].filter(Boolean).join(" · ");
                  return (
                    <button key={track.index} className={`${enabled ? "enabled" : ""} status-${status}`} onClick={() => toggleTrack(track.index)} aria-pressed={enabled}>
                      {enabled ? <Volume2 size={15} /> : <VolumeX size={15} />}
                      <span>{track.title || `Track ${position + 1}`}</span>
                      <small>
                        {status === "preparing" ? <LoaderCircle size={11} className="spin" /> : null}
                        {status === "ready" ? <Check size={11} /> : null}
                        {status === "error" ? "Unavailable" : status === "queued" ? "Queued" : status === "preparing" ? "Preparing" : `${method === "cache" ? "Cached" : method === "copy" ? "Copied" : "Ready"}${details ? ` · ${details}` : ""}`}
                      </small>
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
