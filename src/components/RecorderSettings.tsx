import { useCallback, useEffect, useRef, useState } from "react";
import { api, normalizeAppError } from "../lib/api";
import { formatBytes } from "../lib/format";
import type { RecorderConfig, RecorderMode, RecorderServiceState, RecorderStatus } from "../lib/types";
import { ConfirmDialog } from "./ConfirmDialog";
import { AppIcon } from "./icons";

const MODES: { value: RecorderMode; label: string; help: string }[] = [
  { value: "session", label: "Full session", help: "Records each game from launch to exit as one file; nothing runs between games." },
  { value: "manual", label: "Manual clip only", help: "Keeps a short replay buffer and saves a clip only when you ask." },
  { value: "rolling", label: "Always-on rolling archive", help: "Records continuously into chunks and deletes the oldest at the size cap." },
];
const CODECS = ["h264", "hevc", "av1", "vp8", "vp9", "hevc_hdr", "av1_hdr", "hevc_10bit", "av1_10bit"];
const MIN_FREE_FLOOR_GB = 5;

const lines = (text: string) => text.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);

export function RecorderSettings() {
  const [config, setConfig] = useState<RecorderConfig | null>(null);
  const [lists, setLists] = useState({ audio: "", games: "", focus: "", deny: "" });
  const [status, setStatus] = useState<RecorderStatus | null>(null);
  const [service, setService] = useState<RecorderServiceState | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const saving = useRef(false);

  const pollStatus = useCallback(() => {
    if (document.hidden) return;
    api.recorderStatus().then(setStatus).catch(() => undefined);
  }, []);
  const refreshService = useCallback(() => api.recorderServiceState().then(setService).catch(() => undefined), []);

  function load(next: RecorderConfig) {
    setConfig(next);
    setLists({ audio: next.audio_tracks.join("\n"), games: next.session.game_processes.join("\n"), focus: next.session.focus_app_ids.join("\n"), deny: next.session.deny_appids.join("\n") });
  }

  useEffect(() => {
    api.recorderGetConfig().then(load).catch((reason) => setError(normalizeAppError(reason).message));
    refreshService();
    pollStatus();
    const timer = window.setInterval(pollStatus, 3000);
    return () => window.clearInterval(timer);
  }, [pollStatus, refreshService]);

  async function run(action: () => Promise<string | void>) {
    setError(null);
    setMessage(null);
    try {
      const done = await action();
      if (done) setMessage(done);
    } catch (reason) {
      setError(normalizeAppError(reason).message);
    } finally {
      pollStatus();
    }
  }

  async function save() {
    if (!config || saving.current) return;
    saving.current = true;
    setBusy(true);
    const deny = lines(lists.deny).map(Number);
    if (deny.some((n) => !Number.isInteger(n) || n < 0)) {
      setError("Denied Steam app IDs must be whole numbers, one per line");
      saving.current = false;
      setBusy(false);
      return;
    }
    await run(async () => {
      const next: RecorderConfig = {
        ...config,
        min_free_gb: Math.max(MIN_FREE_FLOOR_GB, config.min_free_gb),
        audio_tracks: lines(lists.audio),
        session: { ...config.session, deny_appids: deny, game_processes: lines(lists.games), focus_app_ids: lines(lists.focus) },
      };
      const reloaded = await api.recorderSaveConfig(next);
      load(next);
      return reloaded ? "Recorder settings saved and applied" : "Recorder settings saved; they apply when the background recorder starts";
    });
    saving.current = false;
    setBusy(false);
  }

  if (!config) return <section className="settings-panel recorder-panel"><h2>Background recorder</h2>{error ? <div className="inline-error">{error}</div> : <p className="muted">Loading…</p>}</section>;

  const set = (patch: Partial<RecorderConfig>) => setConfig({ ...config, ...patch });
  const setSession = (patch: Partial<RecorderConfig["session"]>) => setConfig({ ...config, session: { ...config.session, ...patch } });
  const num = (value: string) => Number(value) || 0;
  const mode = MODES.find((m) => m.value === config.mode);
  const retention = config.session.retention;
  const running = status?.running ?? false;
  const paused = status?.state === "paused";
  const enabled = service?.enabled === "enabled";

  return (
    <section className="settings-panel recorder-panel">
      <h2>Background recorder</h2>
      {!enabled ? (
        <div className="data-status">
          The background recorder is not enabled{service?.enabled === "unavailable" ? " (systemd is not available)" : ""}.
          <br />
          <button className="primary-button" disabled={service?.enabled === "unavailable"} onClick={() => run(async () => { await api.recorderEnableService(); await refreshService(); return "Background recorder enabled and started"; })}>
            <AppIcon name="add" size={17} />Enable background recorder
          </button>
        </div>
      ) : null}
      <div className="data-status" aria-live="polite">
        <strong>{running ? status?.state.replace(/_/g, " ") : "not running"}</strong>
        {running && status ? <>
          {status.detail ? ` · ${status.detail}` : ""} · mode {status.mode}
          <br />Replay buffer {status.replay_seconds ?? 0}s · archive {formatBytes(status.archive_bytes ?? 0)} of {formatBytes(status.cap_bytes ?? 0)} · free {formatBytes(status.free_bytes ?? 0)} (minimum {formatBytes(status.min_free_bytes ?? 0)})
          <br />About {(status.archive_hours_estimate ?? 0).toFixed(1)} h fit in the archive{status.session_game ? ` · playing ${status.session_game}` : ""}
          {status.last_error ? <><br />Last error: {status.last_error}</> : null}
        </> : null}
      </div>
      <div className="recorder-actions">
        <button className="secondary-button" disabled={!running} onClick={() => run(async () => ((await api.recorderSaveClip()) ? "Saving clip…" : "A clip save is already in progress"))}>Save clip</button>
        <button className="secondary-button" disabled={!running} onClick={() => run(() => (paused ? api.recorderResume() : api.recorderPause()))}>{paused ? "Resume" : "Pause"}</button>
        <button className="secondary-button" disabled={!running} onClick={() => setConfirmDelete(true)}>Delete old rolling archive</button>
      </div>

      <label>Mode
        <select value={config.mode} onChange={(event) => set({ mode: event.target.value as RecorderMode })}>
          {MODES.map((m) => <option key={m.value} value={m.value}>{m.label}</option>)}
        </select>
      </label>
      <p className="muted">{mode?.help}</p>
      <label>Recordings folder<input value={config.rec_dir} onChange={(event) => set({ rec_dir: event.target.value })} /></label>
      <label>Clips folder<input value={config.clip_dir ?? ""} onChange={(event) => set({ clip_dir: event.target.value.trim() || null })} placeholder="Defaults to Clips inside the recordings folder" /></label>
      <label>Archive size cap (GB)<input type="number" min="1" value={config.cap_gb} onChange={(event) => set({ cap_gb: num(event.target.value) })} /></label>
      <label>Chunk length (seconds)<input type="number" min="60" value={config.chunk_sec} onChange={(event) => set({ chunk_sec: num(event.target.value) })} /></label>
      <label>Clip length (seconds)<input type="number" min="5" max="3600" value={config.clip_sec} onChange={(event) => set({ clip_sec: num(event.target.value) })} /></label>
      <label>Bitrate (kbps)<input type="number" min="500" max="200000" value={config.bitrate_kbps} onChange={(event) => set({ bitrate_kbps: num(event.target.value) })} /></label>
      <label>Frame rate<input type="number" min="1" max="360" value={config.fps} onChange={(event) => set({ fps: num(event.target.value) })} /></label>
      <label>Capture target<input value={config.capture} onChange={(event) => set({ capture: event.target.value })} placeholder="Monitor name, for example DP-1" /></label>
      <label>Codec
        <select value={config.codec} onChange={(event) => set({ codec: event.target.value })}>
          {CODECS.map((codec) => <option key={codec} value={codec}>{codec}</option>)}
        </select>
      </label>
      <label>Replay buffer storage
        <select value={config.replay_storage} onChange={(event) => set({ replay_storage: event.target.value as "disk" | "ram" })}>
          <option value="disk">Disk (saves memory)</option>
          <option value="ram">Memory (less disk wear)</option>
        </select>
      </label>
      <label>Audio tracks
        <textarea rows={3} value={lists.audio} onChange={(event) => setLists({ ...lists, audio: event.target.value })} placeholder="One audio source per line" />
      </label>
      <label className="toggle-row"><input type="checkbox" checked={config.require_mountpoint} onChange={(event) => set({ require_mountpoint: event.target.checked })} /><span>Only record while the recordings folder is on a mounted drive</span></label>
      <label>Minimum free space (GB)
        <input type="number" min={MIN_FREE_FLOOR_GB} value={config.min_free_gb} onChange={(event) => set({ min_free_gb: num(event.target.value) })} />
      </label>
      <p className="muted">Recording stops before the disk gets this full. The minimum is {MIN_FREE_FLOOR_GB} GB and cannot be turned off.</p>

      <h3>Game sessions</h3>
      <label>Keep session recordings
        <select value={retention.kind} onChange={(event) => setSession({ retention: event.target.value === "days" ? { kind: "days", days: 30 } : event.target.value === "size_cap_gb" ? { kind: "size_cap_gb", gb: 100 } : { kind: "keep_forever" } })}>
          <option value="keep_forever">Forever</option>
          <option value="days">For a number of days</option>
          <option value="size_cap_gb">Up to a total size</option>
        </select>
      </label>
      {retention.kind === "days" ? <label>Days<input type="number" min="1" value={retention.days} onChange={(event) => setSession({ retention: { kind: "days", days: num(event.target.value) } })} /></label> : null}
      {retention.kind === "size_cap_gb" ? <label>Total size (GB)<input type="number" min="1" value={retention.gb} onChange={(event) => setSession({ retention: { kind: "size_cap_gb", gb: num(event.target.value) } })} /></label> : null}
      <label>Discard sessions shorter than (seconds)<input type="number" min="0" value={config.session.min_session_sec} onChange={(event) => setSession({ min_session_sec: num(event.target.value) })} /></label>
      <label>Ignored Steam app IDs
        <textarea rows={2} value={lists.deny} onChange={(event) => setLists({ ...lists, deny: event.target.value })} placeholder="One app ID per line" />
      </label>
      <label>Extra game processes
        <textarea rows={2} value={lists.games} onChange={(event) => setLists({ ...lists, games: event.target.value })} placeholder="One process name per line (non-Steam games)" />
      </label>
      <label>Game window app IDs
        <textarea rows={2} value={lists.focus} onChange={(event) => setLists({ ...lists, focus: event.target.value })} placeholder="One focused-window app ID per line" />
      </label>

      <h3>Feedback and tools</h3>
      <label className="toggle-row"><input type="checkbox" checked={config.sounds} onChange={(event) => set({ sounds: event.target.checked })} /><span>Play sounds</span></label>
      <label className="toggle-row"><input type="checkbox" checked={config.notifications} onChange={(event) => set({ notifications: event.target.checked })} /><span>Show desktop notifications</span></label>
      <label>FFmpeg for clip remuxing<input value={config.ffmpeg_path} onChange={(event) => set({ ffmpeg_path: event.target.value })} /></label>
      <label>gpu-screen-recorder<input value={config.gsr_path} onChange={(event) => set({ gsr_path: event.target.value })} /></label>
      <label>gsr-cli<input value={config.gsr_cli_path} onChange={(event) => set({ gsr_cli_path: event.target.value })} /></label>

      {message ? <p className="data-status">{message}</p> : null}
      {error ? <div className="inline-error">{error}</div> : null}
      <button className="primary-button" onClick={save} disabled={busy}><AppIcon name="save" size={17} />{busy ? "Saving..." : "Save recorder settings"}</button>

      {confirmDelete ? (
        <ConfirmDialog
          title="Delete old rolling archive?"
          description={`This deletes only the rolling rec_*.mkv files in the recordings folder and frees about ${formatBytes(status?.archive_bytes ?? 0)}. Clips and session recordings are not touched.`}
          detail={config.rec_dir}
          confirmLabel="Delete archive"
          onClose={() => setConfirmDelete(false)}
          onConfirm={async () => { const freed = await api.recorderDeleteRollingArchive(); setConfirmDelete(false); setMessage(`Freed ${formatBytes(freed)}`); pollStatus(); }}
        />
      ) : null}
    </section>
  );
}
