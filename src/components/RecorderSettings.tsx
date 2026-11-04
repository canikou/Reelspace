import { useCallback, useEffect, useRef, useState } from "react";
import { api, normalizeAppError } from "../lib/api";
import { formatBytes } from "../lib/format";
import { RECORDER_MODES } from "../lib/recorderUi";
import type { RecorderConfig, RecorderMode, RecorderServiceState, RecorderStatus } from "../lib/types";
import { ConfirmDialog } from "./ConfirmDialog";
import { AppIcon } from "./icons";
import { Segmented, SettingRow, SettingsGroup } from "./SettingRow";

const CODECS = ["h264", "hevc", "av1", "vp8", "vp9", "hevc_hdr", "av1_hdr", "hevc_10bit", "av1_10bit"];
const MIN_FREE_FLOOR_GB = 5;
/** Fired after any recorder config save so the header control and the settings page stay in sync. */
export const RECORDER_CONFIG_EVENT = "reelspace:recorder-config";

type Lists = { audio: string; games: string; focus: string; deny: string };
const lines = (text: string) => text.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
const listsOf = (c: RecorderConfig): Lists => ({ audio: c.audio_tracks.join("\n"), games: c.session.game_processes.join("\n"), focus: c.session.focus_app_ids.join("\n"), deny: c.session.deny_appids.join("\n") });

/** Recorder config state, living in the settings page so one save bar can cover app and recorder settings. */
export function useRecorderSettings(active: boolean) {
  const [config, setConfig] = useState<RecorderConfig | null>(null);
  const [lists, setLists] = useState<Lists>({ audio: "", games: "", focus: "", deny: "" });
  const [saved, setSaved] = useState<string>("");
  const [status, setStatus] = useState<RecorderStatus | null>(null);
  const [service, setService] = useState<RecorderServiceState | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const dirtyRef = useRef(false);

  const snapshotKey = (c: RecorderConfig | null, l: Lists) => JSON.stringify([c, l]);
  const dirty = config !== null && snapshotKey(config, lists) !== saved;
  dirtyRef.current = dirty;

  const pollStatus = useCallback(() => {
    if (document.hidden) return;
    api.recorderStatus().then(setStatus).catch(() => undefined);
  }, []);
  const refreshService = useCallback(() => api.recorderServiceState().then(setService).catch(() => undefined), []);

  const load = useCallback((next: RecorderConfig) => {
    const l = listsOf(next);
    setConfig(next);
    setLists(l);
    setSaved(JSON.stringify([next, l]));
  }, []);

  useEffect(() => {
    if (!active) return;
    api.recorderGetConfig().then(load).catch((reason) => setError(normalizeAppError(reason).message));
    refreshService();
    pollStatus();
    const timer = window.setInterval(pollStatus, 3000);
    const onExternal = () => {
      // The header changed the mode; keep any unsaved edits but adopt the new mode as the saved baseline.
      api.recorderGetConfig().then((next) => {
        if (!dirtyRef.current) return load(next);
        setConfig((c) => (c ? { ...c, mode: next.mode } : c));
        setSaved((s) => { const [c, l] = JSON.parse(s) as [RecorderConfig, Lists]; return JSON.stringify([{ ...c, mode: next.mode }, l]); });
      }).catch(() => undefined);
    };
    window.addEventListener(RECORDER_CONFIG_EVENT, onExternal);
    return () => { window.clearInterval(timer); window.removeEventListener(RECORDER_CONFIG_EVENT, onExternal); };
  }, [active, load, pollStatus, refreshService]);

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

  /** Returns false when validation or the save failed (the error is shown by the page). */
  async function save(): Promise<boolean> {
    if (!config) return true;
    setError(null);
    const deny = lines(lists.deny).map(Number);
    if (deny.some((n) => !Number.isInteger(n) || n < 0)) {
      setError("Denied Steam app IDs must be whole numbers, one per line");
      return false;
    }
    try {
      const next: RecorderConfig = {
        ...config,
        min_free_gb: Math.max(MIN_FREE_FLOOR_GB, config.min_free_gb),
        audio_tracks: lines(lists.audio),
        session: { ...config.session, deny_appids: deny, game_processes: lines(lists.games), focus_app_ids: lines(lists.focus) },
      };
      const applied = await api.recorderSaveConfig(next);
      load(next);
      window.dispatchEvent(new Event(RECORDER_CONFIG_EVENT));
      setMessage(applied ? "Recorder settings saved and applied" : "Recorder settings saved; they apply when the background recorder starts");
      return true;
    } catch (reason) {
      setError(normalizeAppError(reason).message);
      return false;
    }
  }

  return { config, setConfig, lists, setLists, status, service, message, error, setError, setMessage, dirty, save, run, pollStatus, refreshService };
}

export type RecorderSettingsState = ReturnType<typeof useRecorderSettings>;

export function RecorderSections({ r }: { r: RecorderSettingsState }) {
  const { config, status, service, run, pollStatus, refreshService, lists, setLists } = r;
  const [confirmDelete, setConfirmDelete] = useState(false);

  if (!config) return r.error ? <div className="inline-error">{r.error}</div> : <p className="set-note">Loading…</p>;

  const set = (patch: Partial<RecorderConfig>) => r.setConfig({ ...config, ...patch });
  const setSession = (patch: Partial<RecorderConfig["session"]>) => r.setConfig({ ...config, session: { ...config.session, ...patch } });
  const num = (value: string) => Number(value) || 0;
  const retention = config.session.retention;
  const running = status?.running ?? false;
  const paused = status?.state === "paused";
  const enabled = service?.enabled === "enabled";

  return (
    <>
      <SettingsGroup title="Status">
        {!enabled ? (
          <SettingRow label="Background recorder" help={`Not enabled${service?.enabled === "unavailable" ? " (systemd is not available)" : ""}.`}>
            <button className="secondary-button" disabled={service?.enabled === "unavailable"} onClick={() => run(async () => { await api.recorderEnableService(); await refreshService(); return "Background recorder enabled and started"; })}>
              <AppIcon name="add" size={17} />Enable
            </button>
          </SettingRow>
        ) : null}
        <SettingRow
          label={running ? (status?.state.replace(/_/g, " ") ?? "running") : "Not running"}
          help={running && status ? (
            <span aria-live="polite">
              {status.detail ? `${status.detail} · ` : ""}replay buffer {status.replay_seconds ?? 0}s · archive {formatBytes(status.archive_bytes ?? 0)} of {formatBytes(status.cap_bytes ?? 0)} · free {formatBytes(status.free_bytes ?? 0)} (minimum {formatBytes(status.min_free_bytes ?? 0)}) · about {(status.archive_hours_estimate ?? 0).toFixed(1)} h fit{status.session_game ? ` · playing ${status.session_game}` : ""}
              {status.last_error ? <><br />Last error: {status.last_error}</> : null}
            </span>
          ) : undefined}
        >
          <div className="set-buttons">
            <button className="secondary-button" disabled={!running} onClick={() => run(async () => ((await api.recorderSaveClip()) ? "Saving clip…" : "A clip save is already in progress"))}>Save clip</button>
            <button className="secondary-button" disabled={!running} onClick={() => run(() => (paused ? api.recorderResume() : api.recorderPause()))}>{paused ? "Resume" : "Pause"}</button>
            <button className="secondary-button" disabled={!running} onClick={() => setConfirmDelete(true)}>Delete old rolling archive</button>
          </div>
        </SettingRow>
      </SettingsGroup>

      <SettingsGroup title="Recording mode">
        <SettingRow label="Mode" help="The recorder always keeps a replay buffer running. The mode decides what else is saved." stack>
          <div className="mode-list" role="radiogroup" aria-label="Recording mode">
            {RECORDER_MODES.map((m) => (
              <label key={m.value} className={config.mode === m.value ? "mode-option on" : "mode-option"}>
                <input type="radio" name="recorder-mode" value={m.value} checked={config.mode === m.value} onChange={() => set({ mode: m.value as RecorderMode })} />
                <span><strong>{m.label}</strong><small>{m.help}</small></span>
              </label>
            ))}
          </div>
        </SettingRow>
        <SettingRow label="Clip length" help="How much of the replay buffer the clip button saves, in seconds." htmlFor="rec-clip-sec">
          <input id="rec-clip-sec" type="number" min="5" max="3600" value={config.clip_sec} onChange={(event) => set({ clip_sec: num(event.target.value) })} />
        </SettingRow>
        <SettingRow label="Replay buffer storage" help="Disk saves memory; memory means less disk wear.">
          <Segmented name="replay-storage" value={config.replay_storage} options={[{ value: "disk", label: "Disk" }, { value: "ram", label: "Memory" }]} onChange={(replay_storage) => set({ replay_storage })} />
        </SettingRow>
      </SettingsGroup>

      <SettingsGroup title="Capture and quality">
        <SettingRow label="Capture target" help="Monitor name, for example DP-1, or screen." htmlFor="rec-capture">
          <input id="rec-capture" value={config.capture} onChange={(event) => set({ capture: event.target.value })} />
        </SettingRow>
        <SettingRow label="Codec" htmlFor="rec-codec">
          <select id="rec-codec" value={config.codec} onChange={(event) => set({ codec: event.target.value })}>
            {CODECS.map((codec) => <option key={codec} value={codec}>{codec}</option>)}
          </select>
        </SettingRow>
        <SettingRow label="Bitrate (kbps)" htmlFor="rec-bitrate">
          <input id="rec-bitrate" type="number" min="500" max="200000" value={config.bitrate_kbps} onChange={(event) => set({ bitrate_kbps: num(event.target.value) })} />
        </SettingRow>
        <SettingRow label="Frame rate" htmlFor="rec-fps">
          <input id="rec-fps" type="number" min="1" max="360" value={config.fps} onChange={(event) => set({ fps: num(event.target.value) })} />
        </SettingRow>
        <SettingRow label="Audio tracks" help="One audio source per line." htmlFor="rec-audio" stack>
          <textarea id="rec-audio" rows={3} value={lists.audio} onChange={(event) => setLists({ ...lists, audio: event.target.value })} />
        </SettingRow>
      </SettingsGroup>

      <SettingsGroup title="Storage">
        <SettingRow label="Recordings folder" htmlFor="rec-dir" stack>
          <input id="rec-dir" value={config.rec_dir} onChange={(event) => set({ rec_dir: event.target.value })} />
        </SettingRow>
        <SettingRow label="Clips folder" help="Defaults to Clips inside the recordings folder." htmlFor="rec-clip-dir" stack>
          <input id="rec-clip-dir" value={config.clip_dir ?? ""} onChange={(event) => set({ clip_dir: event.target.value.trim() || null })} />
        </SettingRow>
        <SettingRow label="Archive size cap (GB)" help="Rolling archive: the oldest chunks are deleted past this size." htmlFor="rec-cap">
          <input id="rec-cap" type="number" min="1" value={config.cap_gb} onChange={(event) => set({ cap_gb: num(event.target.value) })} />
        </SettingRow>
        <SettingRow label="Chunk length (seconds)" htmlFor="rec-chunk">
          <input id="rec-chunk" type="number" min="60" value={config.chunk_sec} onChange={(event) => set({ chunk_sec: num(event.target.value) })} />
        </SettingRow>
        <SettingRow label="Minimum free space (GB)" help={`Recording stops before the disk gets this full. The minimum is ${MIN_FREE_FLOOR_GB} GB and cannot be turned off.`} htmlFor="rec-minfree">
          <input id="rec-minfree" type="number" min={MIN_FREE_FLOOR_GB} value={config.min_free_gb} onChange={(event) => set({ min_free_gb: num(event.target.value) })} />
        </SettingRow>
        <SettingRow label="Require a mounted drive" help="Only record while the recordings folder is on a mounted drive." htmlFor="rec-mount">
          <input id="rec-mount" type="checkbox" checked={config.require_mountpoint} onChange={(event) => set({ require_mountpoint: event.target.checked })} />
        </SettingRow>
      </SettingsGroup>

      <SettingsGroup title="Game sessions">
        <SettingRow label="Keep session recordings" htmlFor="rec-retention">
          <select id="rec-retention" value={retention.kind} onChange={(event) => setSession({ retention: event.target.value === "days" ? { kind: "days", days: 30 } : event.target.value === "size_cap_gb" ? { kind: "size_cap_gb", gb: 100 } : { kind: "keep_forever" } })}>
            <option value="keep_forever">Forever</option>
            <option value="days">For a number of days</option>
            <option value="size_cap_gb">Up to a total size</option>
          </select>
        </SettingRow>
        {retention.kind === "days" ? <SettingRow label="Days" htmlFor="rec-days"><input id="rec-days" type="number" min="1" value={retention.days} onChange={(event) => setSession({ retention: { kind: "days", days: num(event.target.value) } })} /></SettingRow> : null}
        {retention.kind === "size_cap_gb" ? <SettingRow label="Total size (GB)" htmlFor="rec-gb"><input id="rec-gb" type="number" min="1" value={retention.gb} onChange={(event) => setSession({ retention: { kind: "size_cap_gb", gb: num(event.target.value) } })} /></SettingRow> : null}
        <SettingRow label="Discard sessions shorter than (seconds)" htmlFor="rec-minsession">
          <input id="rec-minsession" type="number" min="0" value={config.session.min_session_sec} onChange={(event) => setSession({ min_session_sec: num(event.target.value) })} />
        </SettingRow>
        <SettingRow label="Ignored Steam app IDs" help="One app ID per line." htmlFor="rec-deny" stack>
          <textarea id="rec-deny" rows={2} value={lists.deny} onChange={(event) => setLists({ ...lists, deny: event.target.value })} />
        </SettingRow>
        <SettingRow label="Extra game processes" help="One process name per line, for non-Steam games." htmlFor="rec-games" stack>
          <textarea id="rec-games" rows={2} value={lists.games} onChange={(event) => setLists({ ...lists, games: event.target.value })} />
        </SettingRow>
        <SettingRow label="Game window app IDs" help="One focused-window app ID per line." htmlFor="rec-focus" stack>
          <textarea id="rec-focus" rows={2} value={lists.focus} onChange={(event) => setLists({ ...lists, focus: event.target.value })} />
        </SettingRow>
      </SettingsGroup>

      <SettingsGroup title="Feedback and tools">
        <SettingRow label="Play sounds" htmlFor="rec-sounds"><input id="rec-sounds" type="checkbox" checked={config.sounds} onChange={(event) => set({ sounds: event.target.checked })} /></SettingRow>
        <SettingRow label="Show desktop notifications" htmlFor="rec-notify"><input id="rec-notify" type="checkbox" checked={config.notifications} onChange={(event) => set({ notifications: event.target.checked })} /></SettingRow>
        <SettingRow label="FFmpeg for clip remuxing" htmlFor="rec-ffmpeg" stack><input id="rec-ffmpeg" value={config.ffmpeg_path} onChange={(event) => set({ ffmpeg_path: event.target.value })} /></SettingRow>
        <SettingRow label="gpu-screen-recorder" htmlFor="rec-gsr" stack><input id="rec-gsr" value={config.gsr_path} onChange={(event) => set({ gsr_path: event.target.value })} /></SettingRow>
        <SettingRow label="gsr-cli" htmlFor="rec-gsrcli" stack><input id="rec-gsrcli" value={config.gsr_cli_path} onChange={(event) => set({ gsr_cli_path: event.target.value })} /></SettingRow>
      </SettingsGroup>

      {r.message ? <p className="data-status" role="status">{r.message}</p> : null}

      {confirmDelete ? (
        <ConfirmDialog
          title="Delete old rolling archive?"
          description={`This deletes only the rolling rec_*.mkv files in the recordings folder and frees about ${formatBytes(status?.archive_bytes ?? 0)}. Clips and session recordings are not touched.`}
          detail={config.rec_dir}
          confirmLabel="Delete archive"
          onClose={() => setConfirmDelete(false)}
          onConfirm={async () => { const freed = await api.recorderDeleteRollingArchive(); setConfirmDelete(false); r.setMessage(`Freed ${formatBytes(freed)}`); pollStatus(); }}
        />
      ) : null}
    </>
  );
}
