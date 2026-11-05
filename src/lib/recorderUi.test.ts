import { describe, expect, it } from "vitest";
import { recorderViewState } from "./recorderUi";
import { pickActiveSection } from "./settingsNav";

const svc = (enabled: string) => ({ enabled, active: "inactive" });

describe("recorderViewState", () => {
  it("is off when the daemon is not running", () => {
    expect(recorderViewState(null, svc("enabled"))).toMatchObject({ on: false, text: "Not running" });
    expect(recorderViewState({ running: false, state: "stopped" }, svc("disabled"))).toMatchObject({ on: false, text: "Not enabled" });
  });
  it("maps live states, treating paused as off", () => {
    expect(recorderViewState({ running: true, state: "recording" }, svc("enabled"))).toMatchObject({ on: true, text: "Recording" });
    expect(recorderViewState({ running: true, state: "replay" }, svc("enabled"))).toMatchObject({ on: true, text: "Replay buffer" });
    expect(recorderViewState({ running: true, state: "paused" }, svc("enabled"))).toMatchObject({ on: false, text: "Paused" });
    expect(recorderViewState({ running: true, state: "waiting_display" }, svc("enabled")).text).toBe("Waiting for display");
    expect(recorderViewState({ running: true, state: "error" }, svc("enabled")).tone).toBe("error");
  });
});

describe("pickActiveSection", () => {
  it("prefers the earliest visible section, else the fallback", () => {
    expect(pickActiveSection(["a", "b", "c"], new Set(["c", "b"]), "a")).toBe("b");
    expect(pickActiveSection(["a", "b"], new Set(), "a")).toBe("a");
  });
});
