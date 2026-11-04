import type { RecorderMode, RecorderServiceState, RecorderStatus } from "./types";

export const RECORDER_MODES: { value: RecorderMode; label: string; help: string }[] = [
  { value: "session", label: "Game sessions", help: "Records each game from launch to close automatically. The clip hotkey or button still saves the last few minutes as a separate clip without touching the session recording." },
  { value: "rolling", label: "Rolling archive", help: "Records all the time in chunks and deletes the oldest once the size cap is reached." },
  { value: "manual", label: "Manual clips only", help: "Keeps only the replay buffer and saves a clip when you ask." },
];

export type RecorderViewState = { on: boolean; text: string; tone: "ok" | "idle" | "warn" | "error" };

/** ON means recording (or buffering); a paused, stopped or absent daemon is OFF. */
export function recorderViewState(status: RecorderStatus | null, service: RecorderServiceState | null): RecorderViewState {
  if (!status?.running) {
    return { on: false, text: service && service.enabled !== "enabled" ? "Not enabled" : "Not running", tone: "idle" };
  }
  switch (status.state) {
    case "recording": return { on: true, text: "Recording", tone: "ok" };
    case "replay": return { on: true, text: "Replay buffer", tone: "ok" };
    case "paused": return { on: false, text: "Paused", tone: "idle" };
    case "waiting_display": return { on: true, text: "Waiting for display", tone: "warn" };
    case "error": return { on: true, text: "Error", tone: "error" };
    default: return { on: true, text: status.state.replace(/_/g, " "), tone: "warn" };
  }
}
