import { StrictMode, memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type MouseEvent as ReactMouseEvent, type PointerEvent as ReactPointerEvent, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Monitor, RotateCw, Settings as SettingsIcon, Square, X } from "lucide-react";
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
    <button className="window-control" title="Minimize" aria-label="Minimize" onClick={() => void run("minimize")}><Minus aria-hidden="true" /></button>
    <button className="window-control" title="Maximize or restore" aria-label="Maximize or restore" onClick={() => void run("maximize")}><Square aria-hidden="true" /></button>
    <button className="window-control close" title="Close to tray" aria-label="Close to tray" onClick={() => void run("close")}><X aria-hidden="true" /></button>
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
    <div className="section-heading"><div><p className="section-kicker">DMX MONITOR</p><h2 id="channel-monitor-title">Universe {universe}</h2></div><div className="inline-actions"><span className="count">{active.length} active</span><button className="secondary-action" aria-pressed={showAll} onClick={() => setShowAll((value) => !value)}>{showAll ? "Active only" : "All 512"}</button>{onClose && <button className="icon-button" title="Close channel monitor" aria-label="Close channel monitor" onClick={onClose}><X aria-hidden="true" /></button>}</div></div>
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
    <aside className="device-inspector" aria-label="Output device"><h2>Output device</h2><strong className={outputIssue ? "is-error" : outputLive ? "is-live" : "is-pending"}>{outputIssue ? "Needs attention" : outputLive ? "Connected" : "Standby"}</strong><dl><dt>Device</dt><dd>{snapshot?.device ?? settings?.device ?? "Auto-select"}</dd><dt>Universe</dt><dd>{snapshot?.universe ?? "—"}</dd><dt>DMX output</dt><dd>{snapshot?.channels ?? "—"} channels</dd><dt>Last frame</dt><dd>{age(snapshot?.last_packet_ms ?? null)}</dd></dl><div className="inspector-actions">{outputIssue && <button className="attention action-button" onClick={onReconnect}><RotateCw aria-hidden="true" />Reconnect output</button>}<button className="primary action-button" disabled={monitorOpening} onClick={onOpenMonitor}><Monitor aria-hidden="true" />{monitorOpening ? "Opening monitor…" : "Open DMX monitor"}</button></div></aside>
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

function Workspace({ children }: { children: ReactNode }) {
  const viewportRef = useRef<HTMLElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const trackRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{ pointerId: number; startY: number; startScrollTop: number; maxScroll: number; maxThumbTravel: number } | null>(null);
  const [scrollbar, setScrollbar] = useState({ visible: false, thumbHeight: 0, thumbTop: 0 });

  const geometry = useCallback(() => {
    const viewport = viewportRef.current;
    const track = trackRef.current;
    if (!viewport || !track) return null;
    const maxScroll = Math.max(0, viewport.scrollHeight - viewport.clientHeight);
    const trackHeight = track.clientHeight;
    const thumbHeight = maxScroll === 0 ? trackHeight : Math.max(32, trackHeight * (viewport.clientHeight / viewport.scrollHeight));
    return { viewport, track, maxScroll, thumbHeight, maxThumbTravel: Math.max(0, trackHeight - thumbHeight) };
  }, []);
  const syncScrollbar = useCallback(() => {
    const next = geometry();
    if (!next) return;
    const thumbTop = next.maxScroll === 0 ? 0 : (next.viewport.scrollTop / next.maxScroll) * next.maxThumbTravel;
    setScrollbar((current) => current.visible === (next.maxScroll > 0) && current.thumbHeight === next.thumbHeight && current.thumbTop === thumbTop ? current : { visible: next.maxScroll > 0, thumbHeight: next.thumbHeight, thumbTop });
  }, [geometry]);

  useLayoutEffect(() => {
    const viewport = viewportRef.current;
    const content = contentRef.current;
    const track = trackRef.current;
    if (!viewport || !content || !track) return;
    const observer = new ResizeObserver(syncScrollbar);
    observer.observe(viewport);
    observer.observe(content);
    observer.observe(track);
    syncScrollbar();
    return () => observer.disconnect();
  }, [syncScrollbar]);

  const updateFromTrackPosition = useCallback((clientY: number) => {
    const next = geometry();
    if (!next || next.maxScroll === 0 || next.maxThumbTravel === 0) return;
    const position = clientY - next.track.getBoundingClientRect().top - next.thumbHeight / 2;
    next.viewport.scrollTop = Math.max(0, Math.min(next.maxScroll, (position / next.maxThumbTravel) * next.maxScroll));
  }, [geometry]);
  const startThumbDrag = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    const next = geometry();
    if (!next || next.maxScroll === 0 || next.maxThumbTravel === 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = { pointerId: event.pointerId, startY: event.clientY, startScrollTop: next.viewport.scrollTop, maxScroll: next.maxScroll, maxThumbTravel: next.maxThumbTravel };
  }, [geometry]);
  const moveThumb = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    const viewport = viewportRef.current;
    if (!drag || !viewport || drag.pointerId !== event.pointerId) return;
    viewport.scrollTop = Math.max(0, Math.min(drag.maxScroll, drag.startScrollTop + ((event.clientY - drag.startY) / drag.maxThumbTravel) * drag.maxScroll));
  }, []);
  const endThumbDrag = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
    if (dragRef.current?.pointerId !== event.pointerId) return;
    dragRef.current = null;
    event.currentTarget.releasePointerCapture(event.pointerId);
  }, []);

  return <div className="workspace-shell"><main ref={viewportRef} className="workspace" tabIndex={0} aria-label="Flux workspace" onScroll={syncScrollbar}><div ref={contentRef}>{children}</div></main><div ref={trackRef} className={`workspace-scrollbar${scrollbar.visible ? "" : " is-hidden"}`} aria-hidden="true" onPointerDown={(event) => { if (event.target === event.currentTarget) updateFromTrackPosition(event.clientY); }}>{scrollbar.visible && <div className="workspace-scrollbar-thumb" style={{ height: scrollbar.thumbHeight, transform: `translateY(${scrollbar.thumbTop}px)` }} onPointerDown={startThumbDrag} onPointerMove={moveThumb} onPointerUp={endThumbDrag} onPointerCancel={endThumbDrag} />}</div></div>;
}

function App() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [isApplying, setIsApplying] = useState(false);
  const [feedback, setFeedback] = useState<Feedback>(null);
  const [monitorOpening, setMonitorOpening] = useState(false);
  const snapshotInFlight = useRef(false);
  const settingsButtonRef = useRef<HTMLButtonElement>(null);
  const settingsDialogRef = useRef<HTMLDivElement>(null);
  const applyingRef = useRef(false);
  const closeSettingsRef = useRef<() => void>(() => {});

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

  const openSettings = useCallback(() => {
    if (!settings || isApplying) return;
    setDraft(settings);
    setSettingsOpen(true);
  }, [isApplying, settings]);
  const closeSettings = useCallback(() => {
    if (isApplying) return;
    setDraft(settings);
    setSettingsOpen(false);
  }, [isApplying, settings]);
  applyingRef.current = isApplying;
  closeSettingsRef.current = closeSettings;

  useEffect(() => {
    if (!settingsOpen) return;
    const dialog = settingsDialogRef.current;
    const focusableSelector = "button:not(:disabled), input:not(:disabled), select:not(:disabled)";
    const focusFirst = () => dialog?.querySelector<HTMLElement>(focusableSelector)?.focus();
    const focusTimer = window.setTimeout(focusFirst, 0);
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        if (!applyingRef.current) closeSettingsRef.current();
        return;
      }
      if (event.key !== "Tab" || !dialog) return;
      const focusable = Array.from(dialog.querySelectorAll<HTMLElement>(focusableSelector));
      if (focusable.length === 0) { event.preventDefault(); dialog.focus(); return; }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      window.clearTimeout(focusTimer);
      document.removeEventListener("keydown", handleKeyDown);
      settingsButtonRef.current?.focus();
    };
  }, [settingsOpen]);

  async function applySettings() {
    if (!draft || !validDraft || !hasChanges) return;
    setIsApplying(true);
    setFeedback({ message: "Applying route configuration…" });
    try { await invoke("save_settings", { settings: draft }); setSettings(draft); setDraft(draft); setSettingsOpen(false); setFeedback({ message: "Route configuration applied." }); }
    catch (error) { setFeedback({ message: `Configuration was not applied: ${String(error)}`, isError: true }); }
    finally { setIsApplying(false); }
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
  const handleTitlebarMouseDown = useCallback((event: ReactMouseEvent<HTMLElement>) => {
    const target = event.target instanceof Element ? event.target : null;
    if (event.button !== 0 || target?.closest("button, input, select, textarea, a, [data-no-window-drag]")) return;
    const currentWindow = getCurrentWindow();
    const action = event.detail === 2 ? currentWindow.toggleMaximize() : currentWindow.startDragging();
    void action.catch((error) => setFeedback({ message: `Window action failed: ${String(error)}`, isError: true }));
  }, []);

  const condition = routeCondition(snapshot);
  return <>
    <div className="app-frame">
      <header className="window-titlebar" onMouseDown={handleTitlebarMouseDown}><div className="brand"><strong>FLUX</strong></div><div className={`status ${condition.className}`}><i className="dot" />{condition.label}</div><div className="window-drag-region" /><div className="titlebar-actions"><button ref={settingsButtonRef} className="titlebar-action" title="Configure route" aria-label="Configure route" aria-haspopup="dialog" aria-expanded={settingsOpen} disabled={!settings} onClick={openSettings}><SettingsIcon aria-hidden="true" /></button><WindowControls onError={(message) => setFeedback({ message, isError: true })} /></div></header>
      <div className="app-shell">
      <Workspace>
        {feedback && <p className={`feedback${feedback.isError ? " error" : ""}`} aria-live="polite">{feedback.message}</p>}
        <RouteSummary snapshot={snapshot} settings={settings} monitorOpening={monitorOpening} onOpenMonitor={() => void openDmxMonitor()} onReconnect={() => void reconnectDevice()} />
      </Workspace></div>
    </div>
    {settingsOpen && <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) closeSettings(); }}><div ref={settingsDialogRef} className="settings-dialog" role="dialog" aria-modal="true" aria-labelledby="route-settings-title" tabIndex={-1}><div className="dialog-heading"><div><p className="section-kicker">CONFIGURATION</p><h2 id="route-settings-title">Route settings</h2><p>Change the saved bridge configuration without restarting Flux.</p></div><button className="dialog-close icon-button" title="Close settings" aria-label="Close settings" disabled={isApplying} onClick={closeSettings}><X aria-hidden="true" /></button></div><div className="settings">
      <label>Listen address<input value={draft?.listen ?? ""} spellCheck={false} disabled={!draft || isApplying} onChange={(event) => setDraft((value) => value && { ...value, listen: event.target.value })} /></label>
      <label>Universe<input value={draft?.universe ?? ""} type="number" min="0" max="32767" disabled={!draft || isApplying} onChange={(event) => setDraft((value) => value && { ...value, universe: Number(event.target.value) })} /></label>
      <label>DMX channels<input value={draft?.channels ?? ""} type="number" min="1" max="512" disabled={!draft || isApplying} onChange={(event) => setDraft((value) => value && { ...value, channels: Number(event.target.value) })} /></label>
      <label>Refresh (Hz)<input value={draft?.fps ?? ""} type="number" min="1" max="44" disabled={!draft || isApplying} onChange={(event) => setDraft((value) => value && { ...value, fps: Number(event.target.value) })} /></label>
      <label className="wide">FTDI device<select value={draft?.device ?? ""} disabled={!draft || isApplying} onChange={(event) => setDraft((value) => value && { ...value, device: event.target.value || null })}><option value="">Auto-select the only device</option>{draft?.device && !selectedDevice && <option value={draft.device}>{draft.device} — not detected</option>}{devices.map((device) => <option key={device.serial} value={device.serial}>{device.serial} — {device.description}{device.port_open ? " (in use)" : ""}</option>)}</select></label>
      </div><p className="hint">{deviceDetail}</p>{feedback?.isError && <p className="dialog-feedback" role="alert">{feedback.message}</p>}<div className="actions"><button disabled={isApplying} onClick={() => void refreshDevices()}>Refresh devices</button><span className="dialog-actions-spacer" />{hasChanges && <span className="changes">Unsaved changes</span>}<button disabled={!hasChanges || isApplying} onClick={() => settings && setDraft(settings)}>Reset</button><button disabled={isApplying} onClick={closeSettings}>Cancel</button><button className="primary" disabled={!validDraft || !hasChanges || isApplying} onClick={() => void applySettings()}>{isApplying ? "Applying…" : "Apply changes"}</button></div></div></div>}
  </>;
}

const isDmxMonitor = window.location.pathname.endsWith("/monitor.html");
createRoot(document.querySelector<HTMLDivElement>("#app")!).render(<StrictMode>{isDmxMonitor ? <MonitorApp /> : <App />}</StrictMode>);
