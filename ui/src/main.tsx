import { StrictMode, memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import "./style.css";

type Snapshot = {
  artnet_state: string;
  listen: string;
  universe: number;
  packets_per_second: number;
  frames_per_second: number;
  last_packet_ms: number | null;
  source: string | null;
  sequence: number | null;
  output_state: string;
  device: string | null;
  refresh_hz: number;
  channels: number;
  dmx: number[];
};

type Settings = {
  listen: string;
  universe: number;
  device: string | null;
  channels: number;
  fps: number;
};

type Device = {
  serial: string;
  description: string;
  device_type: string;
  vendor_id: number;
  product_id: number;
  port_open: boolean;
};

type Feedback = { message: string; isError?: boolean } | null;

const EMPTY_DMX = Array.from({ length: 512 }, () => 0);

function age(value: number | null) {
  if (value === null) return "No packet yet";
  return value < 1000 ? `${value} ms ago` : `${(value / 1000).toFixed(1)} s ago`;
}

function statusName(value: string) {
  return value.toLowerCase().includes("receiving") ? "ACTIVE" : "WAITING";
}

function stateClass(value: string) {
  const state = value.toLowerCase();
  if (state.includes("receiving") || state.includes("connected") || state.includes("active")) return "is-live";
  if (state.includes("error") || state.includes("unavailable")) return "is-error";
  return "is-pending";
}

const ChannelRow = memo(function ChannelRow({ index, value }: { index: number; value: number }) {
  return (
    <div className="channel">
      <span>{String(index + 1).padStart(3, "0")}</span>
      <i><b style={{ width: `${value / 2.55}%` }} /></i>
      <em>{value}</em>
    </div>
  );
});

function ChannelMonitor({ dmx, universe, onClose }: { dmx: number[]; universe: number; onClose: () => void }) {
  return (
    <section className="card channels">
      <div className="section-heading">
        <h2>UNIVERSE {universe} · DMX CHANNELS</h2>
        <button className="icon-button" title="Hide channels" aria-label="Hide DMX channels" onClick={onClose}>×</button>
      </div>
      <div id="channel-list">
        {Array.from({ length: 512 }, (_, index) => <ChannelRow key={index} index={index} value={dmx[index] ?? 0} />)}
      </div>
    </section>
  );
}

function App() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [devices, setDevices] = useState<Device[]>([]);
  const [channelsVisible, setChannelsVisible] = useState(false);
  const [feedback, setFeedback] = useState<Feedback>(null);
  const snapshotInFlight = useRef(false);

  const refreshDevices = useCallback(async () => {
    setFeedback({ message: "Refreshing devices…" });
    try {
      const next = await invoke<Device[]>("available_devices");
      setDevices(next);
      setFeedback({ message: next.length === 0 ? "No FTDI devices found." : `${next.length} FTDI device${next.length === 1 ? "" : "s"} found.` });
    } catch (error) {
      setFeedback({ message: `Unable to list devices: ${String(error)}`, isError: true });
    }
  }, []);

  useEffect(() => {
    let active = true;
    async function initialize() {
      try {
        const [saved, firstSnapshot, detected] = await Promise.all([
          invoke<Settings>("saved_settings"),
          invoke<Snapshot>("runtime_snapshot"),
          invoke<Device[]>("available_devices"),
        ]);
        if (!active) return;
        setSettings(saved);
        setDraft(saved);
        setSnapshot(firstSnapshot);
        setDevices(detected);
      } catch (error) {
        if (active) setFeedback({ message: `Flux UI could not initialize: ${String(error)}`, isError: true });
      }
    }
    void initialize();
    return () => { active = false; };
  }, []);

  useEffect(() => {
    let active = true;
    async function refreshSnapshot() {
      if (snapshotInFlight.current) return;
      snapshotInFlight.current = true;
      try {
        const next = await invoke<Snapshot>("runtime_snapshot");
        if (active) setSnapshot(next);
      } catch (error) {
        if (active) setFeedback({ message: `Unable to read runtime state: ${String(error)}`, isError: true });
      } finally {
        snapshotInFlight.current = false;
      }
    }
    void refreshSnapshot();
    const interval = window.setInterval(() => void refreshSnapshot(), 500);
    return () => {
      active = false;
      window.clearInterval(interval);
    };
  }, []);

  const selectedDevice = useMemo(
    () => devices.find((device) => device.serial === draft?.device),
    [devices, draft?.device],
  );
  const deviceDetail = selectedDevice
    ? `${selectedDevice.device_type} · VID:PID ${selectedDevice.vendor_id.toString(16).padStart(4, "0")}:${selectedDevice.product_id.toString(16).padStart(4, "0")}${selectedDevice.port_open ? " · currently in use" : ""}`
    : devices.length === 0
      ? "No FTDI devices detected. Flux will keep waiting and reconnect automatically."
      : "Flux will select the device automatically only when exactly one is connected.";

  async function applySettings() {
    if (!draft) return;
    setFeedback({ message: "Applying configuration…" });
    try {
      await invoke("save_settings", { settings: draft });
      setSettings(draft);
      setFeedback({ message: "Configuration applied. Waiting for Art-Net on the new route." });
    } catch (error) {
      setFeedback({ message: `Configuration was not applied: ${String(error)}`, isError: true });
    }
  }

  async function reconnectDevice() {
    try {
      await invoke("reconnect_device");
      setFeedback({ message: "Device reconnection requested." });
    } catch (error) {
      setFeedback({ message: `Unable to request reconnection: ${String(error)}`, isError: true });
    }
  }

  const current = snapshot;
  const dmx = current?.dmx ?? EMPTY_DMX;
  const visibleStatus = current ? statusName(current.artnet_state) : "WAITING";
  const isActive = visibleStatus === "ACTIVE";

  return (
    <>
      <header className="topbar">
        <div className="brand">
          <span className="brand-mark">F</span>
          <div><strong>FLUX</strong><span>LIGHTING ROUTER</span></div>
        </div>
        <div className={`status ${isActive ? "is-live" : "is-pending"}`}><i className="dot" />{visibleStatus}</div>
      </header>
      <main className="grid">
        <section className="card hero">
          <div className="hero-copy">
            <p className="eyebrow">LIGHTING DATA BRIDGE</p>
            <h1>Lighting data, in motion.</h1>
            <p>Art-Net is routed continuously to the selected DMX output.</p>
          </div>
          <div className="hero-metrics">
            <div><strong className="metric">{current?.packets_per_second ?? 0}</strong><small>ART-NET PKT/S</small></div>
            <div><strong className="metric">{current?.frames_per_second ?? 0}</strong><small>DMX FRAMES/S</small></div>
          </div>
        </section>
        <section className="card status-card">
          <div className="section-heading"><div><p className="section-kicker">01 · INPUT</p><h2>ART-NET</h2></div><span className={`state-pill ${stateClass(current?.artnet_state ?? "")}`}>{visibleStatus}</span></div>
          <dl>
            <dt>Status</dt><dd className={stateClass(current?.artnet_state ?? "")}>{current?.artnet_state ?? "Starting"}</dd>
            <dt>Listen</dt><dd>{current?.listen ?? "—"}</dd>
            <dt>Universe</dt><dd>{current?.universe ?? "—"}</dd>
            <dt>Last packet</dt><dd>{age(current?.last_packet_ms ?? null)}</dd>
            <dt>Source</dt><dd>{current?.source ?? "—"}</dd>
            <dt>Sequence</dt><dd>{current?.sequence ?? "—"}</dd>
          </dl>
        </section>
        <section className="card configuration-card">
          <div className="section-heading"><div><p className="section-kicker">02 · ROUTE</p><h2>CONFIGURATION</h2></div><button className="icon-button" title="Refresh FTDI devices" aria-label="Refresh FTDI devices" onClick={() => void refreshDevices()}>↻</button></div>
          <div className="settings">
            <label>Listen address<input value={draft?.listen ?? ""} spellCheck={false} disabled={!draft} onChange={(event) => setDraft((value) => value && { ...value, listen: event.target.value })} /></label>
            <label>Universe<input value={draft?.universe ?? ""} type="number" min="0" max="32767" disabled={!draft} onChange={(event) => setDraft((value) => value && { ...value, universe: Number(event.target.value) })} /></label>
            <label>DMX channels<input value={draft?.channels ?? ""} type="number" min="1" max="512" disabled={!draft} onChange={(event) => setDraft((value) => value && { ...value, channels: Number(event.target.value) })} /></label>
            <label>Refresh (Hz)<input value={draft?.fps ?? ""} type="number" min="1" max="44" disabled={!draft} onChange={(event) => setDraft((value) => value && { ...value, fps: Number(event.target.value) })} /></label>
            <label className="wide">FTDI device
              <select value={draft?.device ?? ""} disabled={!draft} onChange={(event) => setDraft((value) => value && { ...value, device: event.target.value || null })}>
                <option value="">Auto-select the only device</option>
                {draft?.device && !selectedDevice && <option value={draft.device}>{draft.device} — not detected</option>}
                {devices.map((device) => <option key={device.serial} value={device.serial}>{device.serial} — {device.description}{device.port_open ? " (in use)" : ""}</option>)}
              </select>
            </label>
          </div>
          <p className="hint">{deviceDetail}</p>
          <div className="actions"><button className="primary" disabled={!draft} onClick={() => void applySettings()}>Apply configuration</button><span className={`feedback${feedback?.isError ? " error" : ""}`} aria-live="polite">{feedback?.message}</span></div>
        </section>
        <section className="card status-card">
          <div className="section-heading"><div><p className="section-kicker">03 · OUTPUT</p><h2>DMX OUTPUT</h2></div><span className={`state-pill ${stateClass(current?.output_state ?? "")}`}>{current?.frames_per_second ? "TRANSMITTING" : "STANDBY"}</span></div>
          <dl>
            <dt>Status</dt><dd className={stateClass(current?.output_state ?? "")}>{current?.output_state ?? "Waiting"}</dd>
            <dt>Device</dt><dd>{current?.device ?? settings?.device ?? "Auto-select"}</dd>
            <dt>Channels</dt><dd>{current?.channels ?? "—"}</dd>
            <dt>Refresh</dt><dd>{current ? `${current.refresh_hz} Hz` : "—"}</dd>
          </dl>
          <div className="actions"><button onClick={() => void reconnectDevice()}>Reconnect device</button><button className="primary" onClick={() => setChannelsVisible((visible) => !visible)}>{channelsVisible ? "Hide DMX channels" : "View DMX channels"}</button></div>
        </section>
        {channelsVisible && <ChannelMonitor dmx={dmx} universe={current?.universe ?? draft?.universe ?? 0} onClose={() => setChannelsVisible(false)} />}
      </main>
    </>
  );
}

createRoot(document.querySelector<HTMLDivElement>("#app")!).render(
  <StrictMode><App /></StrictMode>,
);
