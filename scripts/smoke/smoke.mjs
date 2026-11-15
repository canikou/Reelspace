// Reelspace live smoke driver. Plain CDP over WebSocket against WebView2 on :9333.
// Usage: node smoke.mjs <sourceDir> <outDir>
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [sourceDir, outDir] = process.argv.slice(2);
if (!sourceDir || !outDir) {
  console.error("usage: node smoke.mjs <sourceDir> <outDir>");
  process.exit(2);
}
mkdirSync(outDir, { recursive: true });

const CDP_LIST = "http://127.0.0.1:9333/json";
const GB = 1024 * 1024 * 1024;
const NAV = 'nav[aria-label="Main navigation"]';
const invoke = (cmd, args) => `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}${args ? "," + JSON.stringify(args) : ""})`;

const consoleErrors = [];
const results = [];
let ws;
let nextId = 1;
const pending = new Map();
let originalSettings = null;

// ---------- CDP plumbing ----------

async function connect() {
  const pages = await (await fetch(CDP_LIST)).json();
  const page = pages.find((p) => p.type === "page" && p.url.startsWith("http://127.0.0.1:1420"));
  if (!page) throw new Error("no CDP page at http://127.0.0.1:1420");
  ws = new WebSocket(page.webSocketDebuggerUrl);
  ws.addEventListener("message", (event) => {
    const msg = JSON.parse(event.data);
    if (msg.id) {
      const p = pending.get(msg.id);
      if (!p) return;
      pending.delete(msg.id);
      msg.error ? p.reject(new Error(`${p.method}: ${msg.error.message}`)) : p.resolve(msg.result);
    } else if (msg.method === "Runtime.exceptionThrown") {
      const d = msg.params.exceptionDetails;
      consoleErrors.push({ kind: "exception", text: d.exception?.description ?? d.text, url: d.url, line: d.lineNumber, at: Date.now() });
    } else if (msg.method === "Runtime.consoleAPICalled" && msg.params.type === "error") {
      const text = msg.params.args.map((a) => a.value ?? a.description ?? a.type).join(" ");
      consoleErrors.push({ kind: "console.error", text, at: Date.now() });
    }
  });
  await new Promise((resolve, reject) => {
    ws.addEventListener("open", resolve, { once: true });
    ws.addEventListener("error", () => reject(new Error("websocket error")), { once: true });
  });
  await cdp("Runtime.enable");
  await cdp("Page.enable");
}

function cdp(method, params = {}, timeoutMs = 30_000) {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method}: CDP timeout`)); }, timeoutMs);
    pending.set(id, {
      method,
      resolve: (v) => { clearTimeout(timer); resolve(v); },
      reject: (e) => { clearTimeout(timer); reject(e); }
    });
    ws.send(JSON.stringify({ id, method, params }));
  });
}

// Evaluates an expression (may return a promise). Page-side errors, including non-Error
// rejections from Tauri commands, come back as thrown Errors.
async function evaluate(expression, timeoutMs = 30_000) {
  const wrapped = `(async()=>{try{return {ok:true,v:await (${expression})}}catch(e){return {ok:false,e:(e&&e.message)||JSON.stringify(e)||String(e)}}})()`;
  const r = await cdp("Runtime.evaluate", { expression: wrapped, awaitPromise: true, returnByValue: true }, timeoutMs);
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
  if (!r.result.value.ok) throw new Error(r.result.value.e);
  return r.result.value.v;
}

async function waitFor(expression, timeoutMs, label) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try { if (await evaluate(expression, 10_000)) return; } catch { /* context may be mid-reload */ }
    await sleep(500);
  }
  throw new Error(`timeout ${timeoutMs}ms waiting for ${label}`);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const pad = (n) => String(n).padStart(2, "0");

async function shot(name) {
  const r = await cdp("Page.captureScreenshot", { format: "png" });
  writeFileSync(join(outDir, name), Buffer.from(r.data, "base64"));
}

// ---------- page helpers ----------

const click = (selector) => evaluate(`(()=>{const el=document.querySelector(${JSON.stringify(selector)});if(!el)throw new Error("not found: "+${JSON.stringify(selector)});el.click();return true})()`);

// Native setter + bubbling 'input' event, so React's onChange fires.
const setInput = (selector, value) => evaluate(`(()=>{
  const el=document.querySelector(${JSON.stringify(selector)});
  if(!el)throw new Error("not found: "+${JSON.stringify(selector)});
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set.call(el,${JSON.stringify(value)});
  el.dispatchEvent(new Event("input",{bubbles:true}));
  return el.value})()`);

const navTo = (label) => click(`${NAV} button[aria-label="${label}"]`);
const navReady = `!!document.querySelector('${NAV} button[aria-label="Home"]')`;
const fixtureCard = `[...document.querySelectorAll(".media-card")].find(c=>c.querySelector("h3")?.textContent.includes("Smoke Fixture"))`;

async function reloadPage() {
  await evaluate("window.__smokeMarker=1");
  await cdp("Page.reload");
  await waitFor(`!window.__smokeMarker && ${navReady}`, 60_000, "app shell after reload");
}

async function saveSettingsViaUi() {
  await waitFor(`(()=>{const b=document.querySelector('button[title="Save settings"]');return b&&!b.disabled})()`, 10_000, "Save button enabled");
  await click('button[title="Save settings"]');
  await waitFor(`(()=>{const b=document.querySelector('button[title="Save settings"]');return b&&b.disabled&&!document.querySelector(".settings-actions .unsaved")})()`, 20_000, "save to finish");
}

const backendSettings = async () => (await evaluate(invoke("get_app_state"))).settings;

// ---------- steps ----------

async function stepShell() {
  await waitFor(navReady, 60_000, `${NAV} Home button`);
}

async function stepSetup() {
  const snap = await evaluate(invoke("get_app_state"));
  originalSettings = snap.settings;
  writeFileSync(join(outDir, "original-settings.json"), JSON.stringify(originalSettings, null, 2));
  const settings = { ...snap.settings, sourceFolders: [sourceDir] };
  await evaluate(invoke("save_settings", { settings }));
  await evaluate(invoke("scan_sources"), 150_000);
  await reloadPage();
  // The prompt shows only when sourceFolders is empty. Dismiss through its own button if it appears anyway.
  try {
    await waitFor('!!document.querySelector(".setup-modal")', 3_000, "setup prompt");
    await click('.setup-modal button[aria-label="Dismiss"]');
    return "setup prompt appeared after save; dismissed";
  } catch { /* good: no prompt */ }
}

async function stepLibrary() {
  await navTo("Recordings");
  await waitFor(`!!(${fixtureCard})`, 60_000, '.media-card h3 containing "Smoke Fixture"');
}

async function stepStorageLimit() {
  await navTo("Settings");
  await waitFor('!!document.querySelector("#set-limit")', 15_000, "#set-limit");
  await setInput("#set-limit", "1.5");
  await saveSettingsViaUi();
  const saved = (await backendSettings()).storageLimitBytes;
  if (saved !== 1.5 * GB) throw new Error(`backend storageLimitBytes=${saved}, expected ${1.5 * GB}`);

  await reloadPage();
  await navTo("Settings");
  await waitFor('document.querySelector("#set-limit")?.value==="1.5"', 15_000, '#set-limit value "1.5" after reload');

  await setInput("#set-limit", "");
  await saveSettingsViaUi();
  const cleared = (await backendSettings()).storageLimitBytes;
  if (cleared != null) throw new Error(`storageLimitBytes=${cleared} after clearing, expected null`);
}

async function stepTrimModal() {
  await navTo("Recordings");
  await waitFor(`!!(${fixtureCard})`, 30_000, "fixture card");
  await evaluate(`(${fixtureCard}).querySelector('button[aria-label="Create clip"]').click()`);
  await waitFor('!!document.querySelector(".trim-modal")', 15_000, ".trim-modal");
  await sleep(2_000); // let the video element and track list settle for the screenshot
  await shot(`${pad(5)}-trim-modal-open.png`);
  await click('.trim-modal button[aria-label="Close"]');
  try {
    await waitFor('!document.querySelector(".trim-modal")', 3_000, "modal closed via Close");
  } catch {
    for (const type of ["keyDown", "keyUp"]) {
      await cdp("Input.dispatchKeyEvent", { type, key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
    }
    await waitFor('!document.querySelector(".trim-modal")', 5_000, "modal closed via Escape");
  }
}

// Not a gated step: put the user's real settings back (source folder points at a deleted temp dir otherwise).
async function restoreSettings() {
  if (!originalSettings) return { restored: false, reason: "original settings never captured" };
  try {
    await evaluate(invoke("save_settings", { settings: originalSettings }));
    return { restored: true };
  } catch (e) {
    return { restored: false, reason: String(e.message ?? e) };
  }
}

// ---------- runner ----------

async function runStep(n, name, fn, timeoutMs) {
  const rec = { step: n, name, pass: false };
  let timer;
  try {
    const timeout = new Promise((_, rej) => { timer = setTimeout(() => rej(new Error(`step timeout ${timeoutMs}ms`)), timeoutMs); });
    const note = await Promise.race([fn(), timeout]);
    rec.pass = true;
    if (note) rec.note = note;
  } catch (e) {
    rec.error = String(e.message ?? e);
  } finally {
    clearTimeout(timer);
  }
  try { await shot(`${pad(n)}-${name}.png`); } catch (e) { rec.screenshotError = String(e.message ?? e); }
  console.log(`${rec.pass ? "PASS" : "FAIL"} ${pad(n)} ${name}${rec.error ? " :: " + rec.error : ""}${rec.note ? " (" + rec.note + ")" : ""}`);
  results.push(rec);
  return rec.pass;
}

let cleanup = null;
try {
  await connect();
  const steps = [
    [1, "app-shell", stepShell, 70_000],
    [2, "first-run-setup", stepSetup, 240_000],
    [3, "library-shows-fixture", stepLibrary, 90_000],
    [4, "settings-storage-limit", stepStorageLimit, 120_000],
    [5, "trim-modal", stepTrimModal, 60_000]
  ];
  for (const [n, name, fn, t] of steps) {
    if (n > 1 && !results[0].pass) {
      results.push({ step: n, name, pass: false, error: "skipped: app shell did not render" });
      console.log(`FAIL ${pad(n)} ${name} :: skipped`);
      continue;
    }
    await runStep(n, name, fn, t);
  }
  cleanup = await restoreSettings();
} catch (e) {
  console.error("driver error:", e.message ?? e);
  results.push({ step: 0, name: "driver", pass: false, error: String(e.message ?? e) });
} finally {
  const gated = results.filter((r) => r.step >= 1 && r.step <= 4);
  const allGatedPass = gated.length === 4 && gated.every((r) => r.pass) && !results.some((r) => r.step === 0);
  writeFileSync(join(outDir, "console-errors.json"), JSON.stringify(consoleErrors, null, 2));
  writeFileSync(join(outDir, "result.json"), JSON.stringify({ pass: allGatedPass, steps: results, settingsRestore: cleanup, consoleErrorCount: consoleErrors.length }, null, 2));
  console.log(`${allGatedPass ? "PASS" : "FAIL"}: steps 1-4; console errors: ${consoleErrors.length}; settings restore: ${JSON.stringify(cleanup)}`);
  try { ws?.close(); } catch { /* ignore */ }
  process.exit(allGatedPass ? 0 : 1);
}
