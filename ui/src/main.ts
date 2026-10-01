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

type Settings = { listen: string; universe: number; device: string | null; channels: number; fps: number };
type Device = { serial: string; description: string; device_type: string; vendor_id: number; product_id: number; port_open: boolean };
type ChannelRow = { value: HTMLElement; bar: HTMLElement };

const app = document.querySelector<HTMLDivElement>("#app")!;
let snapshot: Snapshot | null = null;
let devices: Device[] = [];
let channelsVisible = false;
let channelRows: ChannelRow[] = [];
const byId = <T extends HTMLElement>(id: string) => document.querySelector<T>(`#${id}`)!;

function age(value: number | null) {
  if (value === null) return "No packet yet";
  return value < 1000 ? `${value} ms ago` : `${(value / 1000).toFixed(1)} s ago`;
}

function setText(id: string, value: string | number) { byId(id).textContent = String(value); }
function statusName(value: string) { return value.toLowerCase().includes("receiving") ? "ACTIVE" : "WAITING"; }

function renderShell() {
  app.innerHTML = `
    <div class="topbar"><div class="brand"><span class="brand-mark">F</span><span>FLUX</span></div><div id="global-status" class="status"><i class="dot"></i>WAITING</div></div>
    <main class="grid">
      <section class="card hero"><div><p class="eyebrow">LIGHTING DATA BRIDGE</p><h1>Lighting data, in motion.</h1><p>Art-Net is routed continuously to the selected DMX output.</p></div><div class="hero-metrics"><div><strong id="rx-rate" class="metric">0</strong><small>Art-Net pkt/s</small></div><div><strong id="tx-rate" class="metric">0</strong><small>DMX frames/s</small></div></div></section>
      <section class="card"><h2>ART-NET</h2><dl><dt>Status</dt><dd id="artnet-state">Starting</dd><dt>Listen</dt><dd id="listen-state">—</dd><dt>Universe</dt><dd id="universe-state">—</dd><dt>Last packet</dt><dd id="last-packet">No packet yet</dd><dt>Source</dt><dd id="source">—</dd><dt>Sequence</dt><dd id="sequence">—</dd></dl></section>
      <section class="card"><div class="section-heading"><h2>CONFIGURATION</h2><button id="refresh-devices" class="icon-button" title="Refresh FTDI devices">↻</button></div><div class="settings"><label>Listen address<input id="listen" spellcheck="false" /></label><label>Universe<input id="universe" type="number" min="0" max="32767" /></label><label>DMX channels<input id="channel-count" type="number" min="1" max="512" /></label><label>Refresh (Hz)<input id="fps" type="number" min="1" max="44" /></label><label class="wide">FTDI device<select id="device"><option value="">Auto-select the only device</option></select></label></div><p id="device-detail" class="hint">Refresh to detect compatible FTDI devices.</p><div class="actions"><button id="apply" class="primary">Apply configuration</button><span id="feedback" class="feedback" aria-live="polite"></span></div></section>
      <section class="card"><h2>DMX OUTPUT</h2><dl><dt>Status</dt><dd id="output-state">Waiting</dd><dt>Device</dt><dd id="output-device">Auto-select</dd><dt>Channels</dt><dd id="output-channels">—</dd><dt>Refresh</dt><dd id="output-refresh">—</dd></dl><div class="actions"><button id="reconnect">Reconnect device</button><button id="channels" class="primary">View DMX channels</button></div></section>
      <section id="channel-panel" class="card channels" hidden><div class="section-heading"><h2 id="channel-title">UNIVERSE · DMX CHANNELS</h2><button id="hide-channels" class="icon-button" title="Hide channels">×</button></div><div id="channel-list"></div></section>
    </main>`;
  byId<HTMLButtonElement>("apply").addEventListener("click", () => void applySettings());
  byId<HTMLButtonElement>("refresh-devices").addEventListener("click", () => void refreshDevices());
  byId<HTMLButtonElement>("reconnect").addEventListener("click", () => void reconnectDevice());
  byId<HTMLButtonElement>("channels").addEventListener("click", toggleChannels);
  byId<HTMLButtonElement>("hide-channels").addEventListener("click", toggleChannels);
  byId<HTMLSelectElement>("device").addEventListener("change", updateDeviceDetail);
}

function configureForm(settings: Settings) {
  byId<HTMLInputElement>("listen").value = settings.listen;
  byId<HTMLInputElement>("universe").value = String(settings.universe);
  byId<HTMLInputElement>("channel-count").value = String(settings.channels);
  byId<HTMLInputElement>("fps").value = String(settings.fps);
  populateDevices(settings.device);
}

function populateDevices(selected: string | null) {
  const select = byId<HTMLSelectElement>("device");
  select.replaceChildren(new Option("Auto-select the only device", ""));
  for (const device of devices) {
    const suffix = device.port_open ? " (in use)" : "";
    select.add(new Option(`${device.serial} — ${device.description}${suffix}`, device.serial));
  }
  select.value = selected ?? "";
  updateDeviceDetail();
}

function updateDeviceDetail() {
  const serial = byId<HTMLSelectElement>("device").value;
  const device = devices.find((item) => item.serial === serial);
  const detail = device
    ? `${device.device_type} · VID:PID ${device.vendor_id.toString(16).padStart(4, "0")}:${device.product_id.toString(16).padStart(4, "0")}${device.port_open ? " · currently in use" : ""}`
    : devices.length === 0 ? "No FTDI devices detected. Flux will keep waiting and reconnect automatically." : "Flux will select the device automatically only when exactly one is connected.";
  setText("device-detail", detail);
}

function updateSnapshot(next: Snapshot) {
  snapshot = next;
  setText("global-status", statusName(next.artnet_state));
  setText("rx-rate", next.packets_per_second);
  setText("tx-rate", next.frames_per_second);
  setText("artnet-state", next.artnet_state);
  setText("listen-state", next.listen);
  setText("universe-state", next.universe);
  setText("last-packet", age(next.last_packet_ms));
  setText("source", next.source ?? "—");
  setText("sequence", next.sequence ?? "—");
  setText("output-state", next.output_state);
  setText("output-device", next.device ?? "Auto-select");
  setText("output-channels", next.channels);
  setText("output-refresh", `${next.refresh_hz} Hz`);
  if (channelsVisible) updateChannels(next.dmx, next.universe);
}

function setFeedback(message: string, isError = false) {
  const feedback = byId("feedback");
  feedback.textContent = message;
  feedback.classList.toggle("error", isError);
}

async function refreshDevices(selected?: string | null) {
  setFeedback("Refreshing devices…");
  try {
    devices = await invoke<Device[]>("available_devices");
    populateDevices(selected ?? (byId<HTMLSelectElement>("device").value || null));
    setFeedback(devices.length === 0 ? "No FTDI devices found." : `${devices.length} FTDI device${devices.length === 1 ? "" : "s"} found.`);
  } catch (error) { setFeedback(`Unable to list devices: ${String(error)}`, true); }
}

async function applySettings() {
  const settings: Settings = { listen: byId<HTMLInputElement>("listen").value.trim(), universe: Number(byId<HTMLInputElement>("universe").value), device: byId<HTMLSelectElement>("device").value || null, channels: Number(byId<HTMLInputElement>("channel-count").value), fps: Number(byId<HTMLInputElement>("fps").value) };
  setFeedback("Applying configuration…");
  try {
    await invoke("save_settings", { settings });
    setFeedback("Configuration applied. Waiting for Art-Net on the new route.");
  } catch (error) { setFeedback(`Configuration was not applied: ${String(error)}`, true); }
}

async function reconnectDevice() {
  try { await invoke("reconnect_device"); setFeedback("Device reconnection requested."); }
  catch (error) { setFeedback(`Unable to request reconnection: ${String(error)}`, true); }
}

function toggleChannels() {
  channelsVisible = !channelsVisible;
  byId("channel-panel").hidden = !channelsVisible;
  byId<HTMLButtonElement>("channels").textContent = channelsVisible ? "Hide DMX channels" : "View DMX channels";
  if (channelsVisible && snapshot) updateChannels(snapshot.dmx, snapshot.universe);
}

function updateChannels(dmx: number[], universe: number) {
  setText("channel-title", `UNIVERSE ${universe} · DMX CHANNELS`);
  if (channelRows.length === 0) {
    const fragment = document.createDocumentFragment();
    for (let index = 0; index < 512; index += 1) {
      const row = document.createElement("div"); row.className = "channel";
      const number = document.createElement("span"); number.textContent = String(index + 1).padStart(3, "0");
      const track = document.createElement("i"); const bar = document.createElement("b"); track.append(bar);
      const value = document.createElement("em"); row.append(number, track, value); fragment.append(row); channelRows.push({ value, bar });
    }
    byId("channel-list").replaceChildren(fragment);
  }
  channelRows.forEach((row, index) => { const value = dmx[index] ?? 0; row.value.textContent = String(value); row.bar.style.width = `${value / 2.55}%`; });
}

async function refreshSnapshot() {
  try { updateSnapshot(await invoke<Snapshot>("runtime_snapshot")); }
  catch (error) { setFeedback(`Unable to read runtime state: ${String(error)}`, true); }
}

async function initialize() {
  renderShell();
  try {
    const [settings, firstSnapshot] = await Promise.all([invoke<Settings>("saved_settings"), invoke<Snapshot>("runtime_snapshot")]);
    configureForm(settings); updateSnapshot(firstSnapshot); await refreshDevices(settings.device);
  } catch (error) { setFeedback(`Flux UI could not initialize: ${String(error)}`, true); }
  setInterval(() => void refreshSnapshot(), 500);
}

void initialize();
