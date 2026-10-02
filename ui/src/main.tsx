import { StrictMode, memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./style.css";

type Snapshot = {
  artnet_state: string; listen: string; universe: number; packets_per_second: number; frames_per_second: number;
  last_packet_ms: number | null; source: string | null; sequence: number | null; output_state: string;
  device: string | null; refresh_hz: number; channels: number; dmx: number[];
};
type Settings = { listen: string; universe: number; device: string | null; channels: number; fps: number };
type Device = { serial: string; description: string; device_type: string; vendor_id: number; product_id: number; port_open: boolean };
type Feedback = { message: string; isError?: boolean } | null;
type Condition = { label: string; detail: string; className: string };

const EMPTY_DMX = Array.from({ length: 512 }, () => 0);
const age = (value: number | null) => value === null ? "No packet received" : value < 1000 ? `${value} ms ago` : `${(value / 1000).toFixed(1)} s ago`;

function hasOutputIssue(value: string) {
  const state = value.toLowerCase();
  return state.includes("error") || state.includes("unavailable") || state.includes("failed");
}

function routeCondition(snapshot: Snapshot | null): Condition {
  if (!snapshot) return { label: "Loading", detail: "Reading route state", className: "is-pending" };
  if (hasOutputIssue(snapshot.output_state)) return { label: "Needs attention", detail: "DMX output needs recovery", className: "is-error" };
  if (snapshot.last_packet_ms !== null && snapshot.last_packet_ms < 1000 && snapshot.frames_per_second > 0) return { label: "Operational", detail: "Art-Net is reaching DMX output", className: "is-live" };
  return { label: "Waiting for Art-Net", detail: "Output is ready for an incoming route", className: "is-pending" };
}

function WindowControls({ onError }: { onError: (message: string) => void }) {
  async function run(action: "minimize" | "maximize" | "close") {
    try {
      const currentWindow = getCurrentWindow();
      if (action === "minimize") await currentWindow.minimize();
      else if (action === "maximize") await currentWindow.toggleMaximize();
      else await currentWindow.close();
    } catch (error) {
      onError(`Window action failed: ${String(error)}`);
    }
  }

  return <div className="window-controls" aria-label="Window controls">
    <button className="window-control" title="Minimize" aria-label="Minimize" onClick={() => void run("minimize")}><svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3 8h10" /></svg></button>
    <button className="window-control" title="Maximize or restore" aria-label="Maximize or restore" onClick={() => void run("maximize")}><svg viewBox="0 0 16 16" aria-hidden="true"><rect x="4" y="4" width="8" height="8" /></svg></button>
    <button className="window-control close" title="Close to tray" aria-label="Close to tray" onClick={() => void run("close")}><svg viewBox="0 0 16 16" aria-hidden="true"><path d="m4 4 8 8M12 4l-8 8" /></svg></button>
  </div>;
}

const ChannelRow = memo(function ChannelRow({ index, value }: { index: number; value: number }) {
  return <div className="channel"><span>CH {String(index + 1).padStart(3, "0")}</span><i><b style={{ width: `${value / 2.55}%` }} /></i><em>{value}</em></div>;
});

function ChannelMonitor({ dmx, universe, onClose }: { dmx: number[]; universe: number; onClose?: () => void }) {
  const [showAll, setShowAll] = useState(false);
  const active = useMemo(() => dmx.map((value, index) => ({ value, index })).filter(({ value }) => value > 0), [dmx]);
  const channels = showAll ? dmx.map((value, index) => ({ value, index })) : active;
  return <section className="card channels" aria-labelledby="channel-monitor-title">
    <div className="section-heading"><div><p className="section-kicker">DMX MONITOR</p><h2 id="channel-monitor-title">Universe {universe}</h2></div><div className="inline-actions"><span className="count">{active.length} active</span><button aria-pressed={showAll} onClick={() => setShowAll((value) => !value)}>{showAll ? "Active only" : "All 512"}</button>{onClose && <button className="icon-button" title="Close channel monitor" aria-label="Close channel monitor" onClick={onClose}>×</button>}</div></div>
    {channels.length > 0 ? <div id="channel-list">{channels.map(({ index, value }) => <ChannelRow key={index} index={index} value={value} />)}</div> : <p className="empty-state">No active DMX channels yet.</p>}
  </section>;
}

function RouteSummary({ snapshot, settings, monitorOpening, onOpenMonitor, onReconnect }: { snapshot: Snapshot | null; settings: Settings | null; monitorOpening: boolean; onOpenMonitor: () => void; onReconnect: () => void }) {
  const condition = routeCondition(snapshot);
  const inputLive = snapshot?.last_packet_ms !== null && (snapshot?.last_packet_ms ?? Infinity) < 1000;
  const outputIssue = hasOutputIssue(snapshot?.output_state ?? "");
  const outputLive = !outputIssue && (snapshot?.frames_per_second ?? 0) > 0;
  return <section className="route-workspace" aria-labelledby="route-title">
    <div className="route-canvas">
      <div className="route-heading"><div><h1 id="route-title">Art-Net <span>→</span> DMX</h1><p>{condition.detail}</p></div><span className={`state-pill ${condition.className}`}><i className="dot" />{condition.label}</span></div>
      <div className="route-flow">
        <article className="endpoint"><div className="endpoint-heading"><span>INPUT</span><strong className={inputLive ? "is-live" : "is-pending"}>{inputLive ? "Receiving" : "Waiting"}</strong></div><dl><dt>Protocol</dt><dd>Art-Net</dd><dt>Source</dt><dd>{snapshot?.source ?? "—"}</dd><dt>Last packet</dt><dd>{age(snapshot?.last_packet_ms ?? null)}</dd></dl></article>
        <div className={`route-link ${condition.className}`} aria-label="Art-Net to DMX route"><i /><span>→</span></div>
        <article className="endpoint"><div className="endpoint-heading"><span>OUTPUT</span><strong className={outputIssue ? "is-error" : outputLive ? "is-live" : "is-pending"}>{outputIssue ? "Issue" : outputLive ? "Transmitting" : "Standby"}</strong></div><dl><dt>Device</dt><dd>{snapshot?.device ?? settings?.device ?? "Auto-select"}</dd><dt>Frames</dt><dd>{snapshot?.frames_per_second ?? 0} fps</dd><dt>Refresh</dt><dd>{snapshot ? `${snapshot.refresh_hz} Hz` : "—"}</dd></dl></article>
      </div>
      <div className="activity-strip" aria-label="Current route activity"><div><span>Input</span><strong>{snapshot?.packets_per_second ?? 0} pkt/s</strong></div><div><span>Output</span><strong>{snapshot?.frames_per_second ?? 0} fps</strong></div><div><span>Universe</span><strong>{snapshot?.universe ?? "—"}</strong></div><div><span>Channels</span><strong>{snapshot?.channels ?? "—"}</strong></div></div>
    </div>
    <aside className="device-inspector" aria-label="Output device"><h2>Output device</h2><strong className={outputIssue ? "is-error" : outputLive ? "is-live" : "is-pending"}>{outputIssue ? "Needs attention" : outputLive ? "Connected" : "Standby"}</strong><dl><dt>Device</dt><dd>{snapshot?.device ?? settings?.device ?? "Auto-select"}</dd><dt>Universe</dt><dd>{snapshot?.universe ?? "—"}</dd><dt>DMX output</dt><dd>{snapshot?.channels ?? "—"} channels</dd><dt>Last frame</dt><dd>{age(snapshot?.last_packet_ms ?? null)}</dd></dl><div className="inspector-actions">{outputIssue && <button className="attention" onClick={onReconnect}>Reconnect output</button>}<button className="primary" disabled={monitorOpening} onClick={onOpenMonitor}>{monitorOpening ? "Opening monitor…" : "Open DMX monitor"}</button></div></aside>
  </section>;
}

function MonitorApp() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [feedback, setFeedback] = useState<Feedback>(null);
  const snapshotInFlight = useRef(false);

  useEffect(() => {
    let active = true;
    async function refreshSnapshot() {
      if (snapshotInFlight.current) return;
      snapshotInFlight.current = true;
      try {
        const next = await invoke<Snapshot>("runtime_snapshot");
        if (active) setSnapshot(next);
      } catch (error) {
        if (active) setFeedback({ message: `Unable to read DMX state: ${String(error)}`, isError: true });
      } finally { snapshotInFlight.current = false; }
    }
    void refreshSnapshot();
    const interval = window.setInterval(() => void refreshSnapshot(), 500);
    return () => { active = false; window.clearInterval(interval); };
  }, []);

  return <main className="monitor-layout">
    {feedback && <p className={`feedback${feedback.isError ? " error" : ""}`} aria-live="polite">{feedback.message}</p>}
    <ChannelMonitor dmx={snapshot?.dmx ?? EMPTY_DMX} universe={snapshot?.universe ?? 0} />
  </main>;
}

function App() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [feedback, setFeedback] = useState<Feedback>(null);
  const [monitorOpening, setMonitorOpening] = useState(false);
  const snapshotInFlight = useRef(false);

  const refreshDevices = useCallback(async () => {
    setFeedback({ message: "Refreshing FTDI devices…" });
    try { const next = await invoke<Device[]>("available_devices"); setDevices(next); setFeedback({ message: next.length === 0 ? "No FTDI devices found." : `${next.length} FTDI device${next.length === 1 ? "" : "s"} found.` }); }
    catch (error) { setFeedback({ message: `Unable to list devices: ${String(error)}`, isError: true }); }
  }, []);

  useEffect(() => {
    let active = true;
    async function initialize() {
      try {
        const [saved, firstSnapshot, detected] = await Promise.all([invoke<Settings>("saved_settings"), invoke<Snapshot>("runtime_snapshot"), invoke<Device[]>("available_devices")]);
        if (!active) return;
        setSettings(saved); setDraft(saved); setSnapshot(firstSnapshot); setDevices(detected);
      } catch (error) { if (active) setFeedback({ message: `Flux UI could not initialize: ${String(error)}`, isError: true }); }
    }
    void initialize();
    return () => { active = false; };
  }, []);

  useEffect(() => {
    let active = true;
    async function refreshSnapshot() {
      if (snapshotInFlight.current) return;
      snapshotInFlight.current = true;
      try { const next = await invoke<Snapshot>("runtime_snapshot"); if (active) setSnapshot(next); }
      catch (error) { if (active) setFeedback({ message: `Unable to read runtime state: ${String(error)}`, isError: true }); }
      finally { snapshotInFlight.current = false; }
    }
    void refreshSnapshot();
    const interval = window.setInterval(() => void refreshSnapshot(), 500);
    return () => { active = false; window.clearInterval(interval); };
  }, []);

  const selectedDevice = useMemo(() => devices.find((device) => device.serial === draft?.device), [devices, draft?.device]);
  const deviceDetail = selectedDevice ? `${selectedDevice.device_type} · VID:PID ${selectedDevice.vendor_id.toString(16).padStart(4, "0")}:${selectedDevice.product_id.toString(16).padStart(4, "0")}${selectedDevice.port_open ? " · currently in use" : ""}` : devices.length === 0 ? "No FTDI devices detected. Flux will keep waiting and reconnect automatically." : "Flux selects automatically only when exactly one device is connected.";
  const hasChanges = Boolean(draft && settings && (draft.listen !== settings.listen || draft.universe !== settings.universe || draft.device !== settings.device || draft.channels !== settings.channels || draft.fps !== settings.fps));
  const validDraft = Boolean(draft && draft.listen.trim() && Number.isInteger(draft.universe) && draft.universe >= 0 && draft.universe <= 32767 && Number.isInteger(draft.channels) && draft.channels >= 1 && draft.channels <= 512 && Number.isInteger(draft.fps) && draft.fps >= 1 && draft.fps <= 44);

  async function applySettings() {
    if (!draft || !validDraft || !hasChanges) return;
    setFeedback({ message: "Applying route configuration…" });
    try { await invoke("save_settings", { settings: draft }); setSettings(draft); setFeedback({ message: "Route configuration applied." }); }
    catch (error) { setFeedback({ message: `Configuration was not applied: ${String(error)}`, isError: true }); }
  }
  async function reconnectDevice() {
    try { await invoke("reconnect_device"); setFeedback({ message: "Output reconnection requested." }); }
    catch (error) { setFeedback({ message: `Unable to request reconnection: ${String(error)}`, isError: true }); }
  }
  async function openDmxMonitor() {
    if (monitorOpening) return;
    setMonitorOpening(true);
    try { await invoke("open_dmx_monitor"); }
    catch (error) { setFeedback({ message: `Unable to open DMX monitor: ${String(error)}`, isError: true }); }
    finally { setMonitorOpening(false); }
  }

  const condition = routeCondition(snapshot);
  return <>
    <div className="app-frame">
      <header className="window-titlebar"><div className="brand"><strong>FLUX</strong></div><div className={`status ${condition.className}`}><i className="dot" />{condition.label}</div><div className="window-drag-region" data-tauri-drag-region /><WindowControls onError={(message) => setFeedback({ message, isError: true })} /></header>
      <div className="app-shell"><nav className="nav-rail" aria-label="Flux navigation"><button className="nav-item active" aria-current="page" onClick={() => setSettingsOpen(false)}><svg className="nav-icon" viewBox="0 0 16 16" aria-hidden="true"><circle cx="3" cy="8" r="1.5" /><circle cx="13" cy="3" r="1.5" /><circle cx="13" cy="13" r="1.5" /><path d="m4.4 7.2 7.2-3.4M4.4 8.8l7.2 3.4" /></svg>Route</button><button className="nav-item" onClick={() => void openDmxMonitor()}><svg className="nav-icon" viewBox="0 0 16 16" aria-hidden="true"><path d="M3 12V8m3 4V4m3 8V6m3 6V2" /></svg>Monitor</button><button className="nav-item" aria-expanded={settingsOpen} onClick={() => setSettingsOpen((open) => !open)}><svg className="nav-icon" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="2.3" /><path d="M8 2.2v1.3m0 9v1.3m5.8-5.8h-1.3m-9 0H2.2m9.9-4.1-.9.9m-6.2 6.2-.9.9m8 0-.9-.9m-6.2-6.2-.9-.9" /></svg>Settings</button></nav>
      <main className="workspace">
        {feedback && <p className={`feedback${feedback.isError ? " error" : ""}`} aria-live="polite">{feedback.message}</p>}
        <RouteSummary snapshot={snapshot} settings={settings} monitorOpening={monitorOpening} onOpenMonitor={() => void openDmxMonitor()} onReconnect={() => void reconnectDevice()} />
        <section className="configuration"><div className="configuration-toggle"><div><h2>Route settings</h2><p>Change the saved bridge configuration without restarting Flux.</p></div><div className="inline-actions">{hasChanges && <span className="changes">Unsaved changes</span>}<button aria-expanded={settingsOpen} aria-controls="route-settings" onClick={() => setSettingsOpen((open) => !open)}>{settingsOpen ? "Close settings" : "Configure route"}</button></div></div>
        {settingsOpen && <div id="route-settings" className="settings-panel"><div className="settings">
        <label>Listen address<input value={draft?.listen ?? ""} spellCheck={false} disabled={!draft} onChange={(event) => setDraft((value) => value && { ...value, listen: event.target.value })} /></label>
        <label>Universe<input value={draft?.universe ?? ""} type="number" min="0" max="32767" disabled={!draft} onChange={(event) => setDraft((value) => value && { ...value, universe: Number(event.target.value) })} /></label>
        <label>DMX channels<input value={draft?.channels ?? ""} type="number" min="1" max="512" disabled={!draft} onChange={(event) => setDraft((value) => value && { ...value, channels: Number(event.target.value) })} /></label>
        <label>Refresh (Hz)<input value={draft?.fps ?? ""} type="number" min="1" max="44" disabled={!draft} onChange={(event) => setDraft((value) => value && { ...value, fps: Number(event.target.value) })} /></label>
        <label className="wide">FTDI device<select value={draft?.device ?? ""} disabled={!draft} onChange={(event) => setDraft((value) => value && { ...value, device: event.target.value || null })}><option value="">Auto-select the only device</option>{draft?.device && !selectedDevice && <option value={draft.device}>{draft.device} — not detected</option>}{devices.map((device) => <option key={device.serial} value={device.serial}>{device.serial} — {device.description}{device.port_open ? " (in use)" : ""}</option>)}</select></label>
        </div><p className="hint">{deviceDetail}</p><div className="actions"><button onClick={() => void refreshDevices()}>Refresh devices</button><button disabled={!hasChanges} onClick={() => settings && setDraft(settings)}>Reset</button><button className="primary" disabled={!validDraft || !hasChanges} onClick={() => void applySettings()}>Apply changes</button></div></div>}
        </section>
      </main></div>
    </div>
  </>;
}

const isDmxMonitor = window.location.pathname.endsWith("/monitor.html");
createRoot(document.querySelector<HTMLDivElement>("#app")!).render(<StrictMode>{isDmxMonitor ? <MonitorApp /> : <App />}</StrictMode>);
