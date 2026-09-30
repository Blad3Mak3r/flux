import { invoke } from "@tauri-apps/api/core";
import "./style.css";

type Snapshot = {
  artnet_state: string; listen: string; universe: number; packets_per_second: number;
  last_packet_ms: number | null; source: string | null; sequence: number | null;
  output_state: string; device: string | null; refresh_hz: number; channels: number; dmx: number[];
};

const app = document.querySelector<HTMLDivElement>("#app")!;
let channels = false;
let snapshot: Snapshot | null = null;

function age(milliseconds: number | null) {
  return milliseconds === null ? "—" : milliseconds < 1000 ? `${milliseconds} ms ago` : `${(milliseconds / 1000).toFixed(1)} s ago`;
}
function render() {
  if (!snapshot) { app.textContent = "Loading Flux…"; return; }
  if (channels) {
    app.innerHTML = `<header><button id="back">‹ Back</button><strong>UNIVERSE ${snapshot.universe}</strong></header><section class="channels">${snapshot.dmx.map((value,index) => `<div class="channel"><span>${String(index + 1).padStart(3,"0")}</span><i><b style="width:${value / 2.55}%"></b></i><em>${value}</em></div>`).join("")}</section>`;
    document.querySelector("#back")!.addEventListener("click", () => { channels = false; render(); });
    return;
  }
  app.innerHTML = `<header><strong>FLUX</strong><span class="active">● ACTIVE</span></header>
  <section><h2>ART-NET</h2><p class="state">● ${snapshot.artnet_state}</p><dl><dt>Listen</dt><dd>${snapshot.listen}</dd><dt>Universe</dt><dd>${snapshot.universe}</dd><dt>Rate</dt><dd>${snapshot.packets_per_second} pkt/s</dd><dt>Last packet</dt><dd>${age(snapshot.last_packet_ms)}</dd><dt>Source</dt><dd>${snapshot.source ?? "—"}</dd></dl></section>
  <section><h2>DMX OUTPUT</h2><p class="state">● ${snapshot.output_state}</p><dl><dt>Device</dt><dd>${snapshot.device ?? "Auto select"}</dd><dt>Refresh</dt><dd>${snapshot.refresh_hz} Hz</dd></dl><button id="channels">View DMX channels</button></section>
  <footer><button id="reconnect">Reconnect device</button><button class="danger" id="quit">Quit Flux</button></footer>`;
  document.querySelector("#channels")!.addEventListener("click", () => { channels = true; render(); });
  document.querySelector("#reconnect")!.addEventListener("click", () => invoke("reconnect_device"));
  document.querySelector("#quit")!.addEventListener("click", () => invoke("quit_flux"));
}
async function refresh() { snapshot = await invoke<Snapshot>("runtime_snapshot"); render(); }
void refresh();
setInterval(() => void refresh(), 100);
