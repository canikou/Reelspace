import { useCallback, useEffect, useState } from "react";
import { api } from "../lib/api";
import { RECORDER_MODES, recorderViewState } from "../lib/recorderUi";
import type { RecorderConfig, RecorderMode, RecorderServiceState, RecorderStatus } from "../lib/types";
import { RECORDER_CONFIG_EVENT } from "./RecorderSettings";

/** Header control: on/off (resume/pause), mode picker, and save-clip. Renders nothing off Linux. */
export function RecorderToggle({ onError }: { onError: (reason: unknown) => void }) {
  const [supported, setSupported] = useState(false);
  const [status, setStatus] = useState<RecorderStatus | null>(null);
  const [service, setService] = useState<RecorderServiceState | null>(null);
  const [config, setConfig] = useState<RecorderConfig | null>(null);
  const [busy, setBusy] = useState(false);

  const poll = useCallback(() => {
    if (document.hidden) return;
    api.recorderStatus().then(setStatus).catch(() => setStatus(null));
    api.recorderServiceState().then(setService).catch(() => undefined);
  }, []);
  const loadConfig = useCallback(() => { api.recorderGetConfig().then(setConfig).catch(() => undefined); }, []);

  useEffect(() => { api.recorderSupported().then(setSupported).catch(() => undefined); }, []);
  useEffect(() => {
    if (!supported) return;
    poll();
    loadConfig();
    const timer = window.setInterval(poll, 3000);
    const onVisible = () => { if (!document.hidden) poll(); };
    document.addEventListener("visibilitychange", onVisible);
    window.addEventListener(RECORDER_CONFIG_EVENT, loadConfig);
    return () => { window.clearInterval(timer); document.removeEventListener("visibilitychange", onVisible); window.removeEventListener(RECORDER_CONFIG_EVENT, loadConfig); };
  }, [supported, poll, loadConfig]);

  if (!supported) return null;

  const view = recorderViewState(status, service);
  const running = status?.running ?? false;
  const title = status?.last_error ? `${view.text}: ${status.last_error}` : view.text;

  async function act(action: () => Promise<unknown>) {
    setBusy(true);
    try { await action(); } catch (reason) { onError(reason); } finally { setBusy(false); poll(); }
  }

  const toggle = () => act(async () => {
    if (running && view.on) return api.recorderPause();
    if (running) return api.recorderResume();
    return api.recorderEnableService(); // not running: enable and start the user service
  });

  const changeMode = (mode: RecorderMode) => act(async () => {
    // Re-read the config so a concurrent settings edit is not overwritten by a stale copy.
    const current = await api.recorderGetConfig();
    await api.recorderSaveConfig({ ...current, mode });
    setConfig({ ...current, mode });
    window.dispatchEvent(new Event(RECORDER_CONFIG_EVENT));
  });

  return (
    <div className="rec-toggle" role="group" aria-label="Background recorder">
      <button type="button" role="switch" aria-checked={view.on} className={`switch tone-${view.tone}`} disabled={busy} onClick={toggle} title={view.on ? "Pause recording" : running ? "Resume recording" : "Start the background recorder"} aria-label="Background recorder">
        <i aria-hidden="true" />
      </button>
      <span className={`rec-state tone-${view.tone}`} title={title} aria-live="polite">{view.text}</span>
      <select aria-label="Recording mode" value={config?.mode ?? ""} disabled={busy || !config} onChange={(event) => changeMode(event.target.value as RecorderMode)} title="Recording mode">
        {config ? null : <option value="" />}
        {RECORDER_MODES.map((m) => <option key={m.value} value={m.value} title={m.help}>{m.label}</option>)}
      </select>
      <button type="button" className="secondary-button rec-save" disabled={!running || busy} onClick={() => act(async () => { if (!(await api.recorderSaveClip())) throw new Error("A clip save is already in progress"); })} title="Save the last few minutes as a clip">Save clip</button>
    </div>
  );
}
