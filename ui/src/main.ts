import { invoke } from "@tauri-apps/api/core";
import "./style.css";

type Snapshot = {
  artnet_state: string; listen: string; universe: number; packets_per_second: number;
  last_packet_ms: number | null; source: string | null; sequence: number | null;
  output_state: string; device: string | null; refresh_hz: number; channels: number; dmx: number[];
};
const app = document.querySelector<HTMLDivElement>("#app")!;
let snapshot: Snapshot | null = null;
let showChannels = false;
let settingsLoaded = false;

function age(value: number | null) {
  if (value === null) return "No packet yet";
  return value < 1000 ? `${value} ms ago` : `${(value / 1000).toFixed(1)} s ago`;
}
function render() {
  if (!snapshot) { app.textContent = "Loading Flux…"; return; }
  const status = snapshot.artnet_state.includes("online") || snapshot.artnet_state.includes("receiving") ? "ACTIVE" : "WAITING";
  app.innerHTML = `<div class="topbar"><div class="brand"><span class="brand-mark">F</span>FLUX</div><div class="status"><i class="dot"></i>${status}</div></div>
  <main class="grid">
    <section class="card hero"><div><h1>Lighting data, in motion.</h1><p>Art-Net universe ${snapshot.universe} is routed to your selected DMX output.</p></div><div><div class="metric">${snapshot.packets_per_second}</div><small>packets / second</small></div></section>
    <section class="card"><h2>ART-NET</h2><dl><dt>Status</dt><dd>${snapshot.artnet_state}</dd><dt>Listen</dt><dd>${snapshot.listen}</dd><dt>Universe</dt><dd>${snapshot.universe}</dd><dt>Last packet</dt><dd>${age(snapshot.last_packet_ms)}</dd><dt>Source</dt><dd>${snapshot.source ?? "—"}</dd><dt>Sequence</dt><dd>${snapshot.sequence ?? "—"}</dd></dl></section>
    <section class="card"><h2>CONFIGURATION</h2><div class="settings"><label>Listen address<input id="listen" value="${snapshot.listen}" /></label><label>Universe<input id="universe" type="number" min="0" max="32767" value="${snapshot.universe}" /></label><label>DMX channels<input id="channel-count" type="number" min="1" max="512" value="${snapshot.channels}" /></label><label>Refresh (Hz)<input id="fps" type="number" min="1" max="44" value="${snapshot.refresh_hz}" /></label><label>FTDI serial<input id="device" value="${snapshot.device ?? ""}" placeholder="Auto select" /></label></div><div class="actions"><button class="primary" id="apply">Apply configuration</button></div></section>
    <section class="card"><h2>DMX OUTPUT</h2><dl><dt>Status</dt><dd>${snapshot.output_state}</dd><dt>Device</dt><dd>${snapshot.device ?? "Auto select"}</dd><dt>Channels</dt><dd>${snapshot.channels}</dd><dt>Refresh</dt><dd>${snapshot.refresh_hz} Hz</dd></dl><div class="actions"><button id="reconnect">Reconnect</button><button class="primary" id="channels">${showChannels ? "Hide channels" : "View DMX channels"}</button></div></section>
    ${showChannels ? `<section class="card channels"><h2>UNIVERSE ${snapshot.universe} · DMX CHANNELS</h2>${snapshot.dmx.map((value,index) => `<div class="channel"><span>${String(index + 1).padStart(3,"0")}</span><i><b style="width:${value / 2.55}%"></b></i><em>${value}</em></div>`).join("")}</section>` : ""}</main>`;
  document.querySelector("#reconnect")?.addEventListener("click", () => void invoke("reconnect_device"));
  document.querySelector("#apply")?.addEventListener("click", () => {
    const read = (id: string) => (document.querySelector<HTMLInputElement>(id)?.value ?? "");
    void invoke("save_settings", { settings: {
      listen: read("#listen"), universe: Number(read("#universe")), device: read("#device") || null,
      channels: Number(read("#channel-count")), fps: Number(read("#fps"))
    }}).then(() => window.alert("Configuration saved."));
  });
  document.querySelector("#channels")?.addEventListener("click", () => { showChannels = !showChannels; render(); });
}
async function refresh() { snapshot = await invoke<Snapshot>("runtime_snapshot"); render(); }
void refresh();
setInterval(() => void refresh(), 200);
