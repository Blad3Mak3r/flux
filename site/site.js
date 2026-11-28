// FLUX landing: language selector and demo clock.
// English lives in the HTML (it reads fine without JS); Spanish is swapped in from ES below.
"use strict";

const ES = {
  title: "FLUX — Control de shows y visuales en directo",
  desc: "Monta shows en una línea de tiempo, usa timecode LTC o MTC y mezcla visuales en directo en modo VJ. FLUX es una app de escritorio para Windows y Linux.",
  skip: "Saltar al contenido", docs: "Documentación", download: "Descargar",
  lang_label: "Idioma", nav_label: "Navegación principal",
  h1a: "Control del show.", h1b: "Visuales en directo.",
  lead: "Crea líneas de tiempo y ejecuta cues con timecode en CUE. Lanza y mezcla visuales sobre la marcha en VJ. Dos modos en una app de escritorio para espectáculos en directo.",
  download_flux: "Descargar FLUX", explore: "Explorar los modos",
  hero_alt: "Vista Edit de FLUX con audio, vídeo, notas, MIDI y OSC en una línea de tiempo",
  hero_caption: "Modo CUE · Línea de tiempo de un show de ejemplo",
  h_modes: "Tres formas de trabajar. Dos modos.",
  mode_timeline: "Monta una línea de tiempo", mode_timeline_p: "Ordena audio, vídeo y cues con un mismo reloj de timecode.",
  mode_live: "Ejecuta los cues", mode_live_p: "Pulsa Ctrl+1 para Edit y Ctrl+2 para Live. En Live ves el cue en marcha, el que está en espera y cuánto falta para el siguiente.",
  mode_vj: "Mezcla visuales en directo", mode_vj_p: "Lanza clips a mano, mezcla capas y envía la composición al escenario.",
  h_vj: "Lanza y mezcla visuales en modo VJ",
  vj_p: "Crea un show VJ para improvisar visuales desde una rejilla de clips. Lanza un clip o una columna entera, mezcla vídeo, imágenes, generadores y entradas en directo por capas y envía la composición a las pantallas.",
  vj_alt: "Carta de ajuste de FLUX VJ con dos slices, rejillas de módulos numerados y guías de alineación",
  vj_caption: "Carta de ajuste por slice de VJ capturada de una salida de vídeo de FLUX.",
  vj_decks_t: "Decks y capas", vj_decks: "Organiza los clips en decks y mantén las capas globales disponibles al cambiar de deck.",
  vj_mix_t: "Mezcla en directo", vj_mix: "Encadena efectos, elige modos de fusión y mezcla las capas que asignes a cada lado del crossfader A/B.",
  vj_control_t: "Control y tempo", vj_control: "Asigna controles MIDI, OSC, DMX o teclas. Sincroniza el tempo con Ableton Link o reloj MIDI.",
  vj_mapping_t: "Mapping de vídeo", vj_mapping: "Envía la mezcla a varias pantallas con slices, bordes suaves, máscaras y cartas de ajuste por slice.",
  vj_guide: "Leer la guía de VJ", timeline_guide: "Leer la guía de edición", setlist_guide: "Leer la guía de setlists",
  o_version: "Versión", o_platforms: "Plataformas", o_platforms_v: "Windows, Linux", o_format: "Archivo de proyecto",
  h_timecode_demo: "Prueba el reloj de timecode",
  start_line: "INICIO 01:00:00:00", k_stop: "Stop", k_play: "Play", k_pause: "Pausa",
  rate_legend: "Velocidad de fotogramas", jump: "Ir a 01:00:58:00",
  fig1: "Simulación de timecode en el navegador.",
  tc_label: "Visor de timecode de demostración", transport_label: "Transporte",
  h_outputs: "Un reloj, todas las salidas",
  outputs_p: "El reloj de muestras de la interfaz de audio lo mueve todo. La reproducción, LTC, MTC y los cues se leen de la misma posición, así que la hora que ves es la hora que envías.",
  s_ltc: "Timecode lineal a través de una interfaz de audio.", s_mtc: "Timecode MIDI a través de un puerto MIDI.",
  s_rates_t: "Velocidades", s_rates: "24, 25, 29,97 drop-frame y 30 fps.",
  s_cues_t: "Cues", s_cues: "Las pistas MIDI (notas, control y cambio de programa, MIDI Show Control, SysEx) y las pistas OSC (UDP) se disparan al compás del timecode. Al saltar de posición nunca se repiten los eventos que se saltaron.",
  s_hw_t: "Hardware", s_hw: "Un dispositivo ausente se explica y se puede arreglar, y nunca impide abrir un proyecto.",
  flow_label: "Camino de la señal: el proyecto alimenta el reloj maestro, el transporte y los cues, que alimentan LTC, MTC, el audio del programa y los cues MIDI y OSC. Un control remoto mueve el transporte, dibujado discontinuo.",
  f_project: "Proyecto", f_project_s: "pistas y audio", f_clock: "Reloj", f_clock_s: "muestras de la interfaz",
  f_transport: "Transporte", f_transport_s: "play, pausa, stop, saltar", f_cues: "Cues", f_cues_s: "derivados del proyecto",
  f_ltc: "interfaz de audio", f_mtc: "puerto MIDI", f_audio: "Audio", f_audio_s: "audio del programa",
  f_cueout: "Cues", f_cueout_s: "MIDI y OSC", f_remote: "Remoto",
  f_remote_s: "MIDI, OSC",
  scroll_diagram: "Desplázate horizontalmente para ver todo el diagrama.",
  fig2: "Camino de la señal. La línea discontinua mueve el reloj; nunca lo alimenta.",
  h_edit: "Monta el show en la línea de tiempo",
  edit_p: "Organiza el audio, el vídeo y los cues juntos, desde la intro hasta la última canción.",
  e1_t: "Líneas de tiempo", e1: "Varias por proyecto, cada una con sus pistas, su velocidad de fotogramas y su desfase de inicio.",
  e2_t: "Pistas", e2: "Audio, vídeo, notas, MIDI y OSC.",
  e3_t: "Audio", e3: "Importa wav, aiff, flac, mp3 y ogg, mezclados con el reloj del dispositivo de audio, con detección de tempo y clic de metrónomo.",
  e4_t: "Voz del operador", e4: "Escribe diálogos y asocia voz generada (ElevenLabs, con tu propia clave de API) o audio importado y grabado. La voz generada se archiva en el proyecto, así que suena sin conexión.",
  e5_t: "Enrutado", e5: "Envía cada pista a un destino con nombre o a Silencio. Cada ordenador asigna los destinos a sus propias interfaces y canales.",
  h_live: "Ejecuta los cues desde la cabina",
  live_p: "Pulsa Ctrl+1 para Edit y Ctrl+2 para Live. En Live ves el cue en marcha, el que está en espera y cuánto falta para el siguiente.",
  live_alt: "Vista Live de FLUX: un timecode grande con el transporte arriba, el cue en marcha con su cuenta atrás a la izquierda y la lista de cues a la derecha",
  fig4: "La vista Live ejecutando un setlist preparado del mismo show. Una captura real de la app.",
  l1_t: "El tiempo primero", l1: "Un timecode que se lee de un vistazo, con el estado del transporte escrito al lado.",
  l2_t: "El cue en marcha", l2: "El cue que suena con su cuenta atrás, lo que está en espera y las notas del operador.",
  l3_t: "Toda la lista de cues", l3: "Sigue todo el orden del show con el cue actual y los siguientes señalados.",
  h_video: "Llévalo a las pantallas",
  video_p: "Haz mapping del vídeo de la línea de tiempo o de una composición VJ en monitores y proyectores. Divide la imagen en slices, ajusta cada uno a su superficie, funde los proyectores que se solapan y aplica máscaras donde haga falta.",
  outputs_alt: "Dos salidas de proyector una al lado de la otra: cada una muestra su mitad de la imagen y se funde a negro donde se solapan, y la izquierda tiene un rectángulo enmascarado",
  fig5v: "Dos salidas de proyector del show de ejemplo. Cada una se funde donde se solapa con la otra, así que las dos forman una sola imagen, y la izquierda enmascara una puerta del escenario. Ventanas de salida reales de la app.",
  mapping_alt: "El editor de mapping: la imagen con un slice a la izquierda, la pantalla con las esquinas del slice a la derecha y campos con el slice en píxeles y su borde suave",
  fig6: "El editor de mapping de una pantalla. Arrastra o escribe cada valor en píxeles. Una captura real de la app.",
  v1_t: "Pantallas", v1: "Hasta ocho, cada una en un monitor o proyector propio. Pertenecen al show; en qué monitor se abre cada una pertenece al ordenador.",
  v2_t: "Slices", v2: "Cualquier parte de la imagen, colocada con cuatro esquinas que la inclinan para encajar en una superficie vista en ángulo.",
  v3_t: "Borde suave", v3: "Funde cada lado de un slice con una curva ajustable, para que los proyectores que se solapan se mezclen.",
  v4_t: "Máscaras", v4: "Formas de hasta 32 puntos que dejan en negro lo que tapan, o todo lo demás.",
  v5_t: "Carta de ajuste", v5: "Una carta de ajuste con cada slice dibujado y con nombre, el tamaño de la imagen, la hora y un barrido, para alinear los proyectores.",
  h_setlists: "Ordena las líneas de tiempo en un show",
  setlists_p: "Un setlist ordena líneas de tiempo en un show, con entradas repetidas u omitidas, transiciones manuales o automáticas, esperas y bucles con devamp. El audio, los cues y el timecode se repiten juntos, guiados por una acción GO en Live.",
  sl1_t: "Transiciones", sl1: "Manuales o automáticas, con retardos y esperas exactos.",
  sl2_t: "Bucles", sl2: "Hasta GO o un número de vueltas, con salida al terminar la vuelta actual.",
  sl3_t: "Sin copias", sl3: "Las entradas referencian líneas de tiempo en vez de copiarlas, así que una edición se ve en todos los sitios donde se usa.",
  h_remote: "Contrólalo desde un controlador",
  remote_p: "Inicia, para, muévete entre cues y líneas de tiempo y cambia de vista desde un controlador MIDI (pads que se aprenden al tocarlos) o desde OSC por UDP y TCP, por ejemplo un Stream Deck. FLUX sigue siendo el reloj maestro: el control remoto lo mueve y nunca sigue a otro.",
  remote_p2: "Se configura en Preferencias y pertenece al ordenador, no al proyecto.",
  osc_cap: "Algunas de las direcciones OSC fijas", osc_addr: "Dirección", osc_does: "Hace",
  osc_play: "Reproducir", osc_stop: "Parar y volver al inicio", osc_go: "Ejecutar la acción GO",
  osc_next: "Ir al siguiente cue", osc_goto: "Ir al cue número n", osc_view: "Cambiar a la vista Live",
  h_project: "Archivos de proyecto y medios",
  project_p: 'Elige CUE o VJ al crear un proyecto <span class="m">.flx</span>. FLUX guarda el show en el proyecto y enlaza los medios del disco por defecto. Usa Archivo → Medios para volver a enlazar los archivos o reunirlos al mover un show. Los proyectos <span class="m">.fxtc</span> antiguos siguen abriéndose.',
  tree_label: "Contenido de un archivo .flx", tree1: "datos del show y enlaces a medios", tree2: "audio incrustado, si lo hay", tree3: "vídeo o imágenes incrustados, si los hay",
  fig5: "Estructura de un archivo de proyecto.",
  h_specs: "Especificaciones y estado",
  sp4_t: "Audio importable", sp8_t: "Vídeo importable",
  sp5_t: "Plataformas", sp5: "Windows (instalador), Linux (AppImage, DEB, RPM).",
  sp6_t: "Idiomas", sp6: "Inglés y español.",
  sp7_t: "Diagnóstico", sp7: "Un registro rotativo y un zip con un clic con los registros y un informe del sistema y los dispositivos, desde el menú Ayuda.",
  st_on: "Funciones",
  st1: "Proyectos .flx con varias líneas de tiempo", st2: "Pistas de audio y notas, detección de tempo, metrónomo",
  st3: "Generación de LTC y MTC, con un transporte completo", st4: "Pistas de cues MIDI y OSC", st5: "Vista Live y setlists con GO",
  st6: "Pistas de voz del operador", st7: "Control remoto MIDI y OSC, con pads iluminados",
  st8: "Pistas de vídeo en varias pantallas, con slices, bordes suaves y máscaras",
  st9: "Lanzamiento de clips, mezcla por capas y mapping de VJ",
  st_off: "Aún no construido",
  sn2: "Cues Art-Net", sn3: "Seguir un timecode externo", sn5: "Versiones para macOS",
  h_order: "Consigue FLUX",
  order_p: "FLUX está en desarrollo temprano. Elige el instalador para tu sistema en la página de la última versión: el instalador de Windows, o el AppImage, DEB o RPM de Linux.",
  source: "Ejecútalo desde el código", docs_all: "Lee la documentación", issues: "Informa de un problema",
  license: "Con licencia GNU General Public License v3.0 o posterior.",
  // Used by the demo clock
  w_stopped: "PARADO", w_playing: "REPRODUCIENDO", w_paused: "EN PAUSA",
  rn_24: "Sin drop: se cuentan todos los números de fotograma, 24 por segundo.",
  rn_25: "Sin drop: se cuentan todos los números de fotograma, 25 por segundo.",
  "rn_29.97": "Drop-frame: los números de fotograma 00 y 01 se saltan al inicio de cada minuto, salvo cada décimo, para que la etiqueta siga el reloj de pared. No se pierde ninguna imagen. Salta al borde del minuto y míralo.",
  rn_30: "Sin drop: se cuentan todos los números de fotograma, 30 por segundo.",
};

const EN = {
  w_stopped: "STOPPED", w_playing: "PLAYING", w_paused: "PAUSED",
  rn_24: "Non-drop: every frame number is counted, 24 per second.",
  rn_25: "Non-drop: every frame number is counted, 25 per second.",
  "rn_29.97": "Drop-frame: frame numbers 00 and 01 are skipped at the start of each minute, except every tenth, so the label keeps pace with the wall clock. No picture is dropped. Jump to the minute edge and watch.",
  rn_30: "Non-drop: every frame number is counted, 30 per second.",
};

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

function store(get, set) {
  return {
    get() { try { return get(); } catch { return null; } },
    set(v) { try { set(v); } catch { /* storage blocked: the page works without it */ } },
  };
}
const saved = store(() => localStorage.getItem("flux-site-lang"), (v) => localStorage.setItem("flux-site-lang", v));

// ---------------------------------------------------------------- language
const original = {
  title: document.title,
  desc: $('meta[name="description"]').content,
};
for (const el of $$("[data-i18n]")) el.dataset.en = el.innerHTML;
for (const el of $$("[data-i18n-aria]")) el.dataset.enAria = el.getAttribute("aria-label");
for (const el of $$("[data-i18n-alt]")) el.dataset.enAlt = el.getAttribute("alt");

let lang = "en";
function t(key) { return (lang === "es" ? ES[key] : EN[key]) ?? EN[key]; }

function setLang(next, persist) {
  lang = next === "es" ? "es" : "en";
  const es = lang === "es";
  document.documentElement.lang = lang;
  for (const el of $$("[data-i18n]")) el.innerHTML = es && ES[el.dataset.i18n] != null ? ES[el.dataset.i18n] : el.dataset.en;
  for (const el of $$("[data-i18n-aria]")) el.setAttribute("aria-label", es && ES[el.dataset.i18nAria] != null ? ES[el.dataset.i18nAria] : el.dataset.enAria);
  for (const el of $$("[data-i18n-alt]")) el.setAttribute("alt", es && ES[el.dataset.i18nAlt] != null ? ES[el.dataset.i18nAlt] : el.dataset.enAlt);
  document.title = es ? ES.title : original.title;
  $('meta[name="description"]').content = es ? ES.desc : original.desc;
  for (const b of $$(".lang button")) b.setAttribute("aria-pressed", String(b.dataset.lang === lang));
  if (persist) saved.set(lang);
  demo?.refreshText();
}

// ------------------------------------------------------------- demo clock
const RATES = {
  "24": { fps: 24, real: 24, df: false, label: "24 FPS" },
  "25": { fps: 25, real: 25, df: false, label: "25 FPS" },
  "29.97": { fps: 30, real: 30000 / 1001, df: true, label: "29.97 DF" },
  "30": { fps: 30, real: 30, df: false, label: "30 FPS" },
};

/** Frame count of h:m:s:f at `rate`; drop-frame skips labels 00 and 01 each minute except every tenth. */
function toFrames(h, m, s, f, rate) {
  let n = ((h * 60 + m) * 60 + s) * rate.fps + f;
  if (rate.df) {
    const minutes = h * 60 + m;
    n -= 2 * (minutes - Math.floor(minutes / 10));
  }
  return n;
}

/** [h, m, s, f] for a frame count at `rate`. */
function toTimecode(frames, rate) {
  let n = frames;
  if (rate.df) {
    const tenMinutes = 17982; // frames in ten drop-frame minutes
    const d = Math.floor(n / tenMinutes);
    const r = n % tenMinutes;
    n += 18 * d + 2 * Math.floor(Math.max(r - 2, 0) / 1798);
  }
  const fps = rate.fps;
  return [Math.floor(n / (fps * 3600)) % 24, Math.floor(n / (fps * 60)) % 60, Math.floor(n / fps) % 60, n % fps];
}

// 5x7 dot-matrix glyphs
const GLYPHS = {
  0: ["01110", "10001", "10011", "10101", "11001", "10001", "01110"],
  1: ["00100", "01100", "00100", "00100", "00100", "00100", "01110"],
  2: ["01110", "10001", "00001", "00010", "00100", "01000", "11111"],
  3: ["11110", "00001", "00001", "01110", "00001", "00001", "11110"],
  4: ["00010", "00110", "01010", "10010", "11111", "00010", "00010"],
  5: ["11111", "10000", "11110", "00001", "00001", "10001", "01110"],
  6: ["00110", "01000", "10000", "11110", "10001", "10001", "01110"],
  7: ["11111", "00001", "00010", "00100", "01000", "01000", "01000"],
  8: ["01110", "10001", "10001", "01110", "10001", "10001", "01110"],
  9: ["01110", "10001", "10001", "01111", "00001", "00010", "01100"],
  ":": ["0", "0", "1", "0", "1", "0", "0"],
};

const demo = (() => {
  const canvas = $("#tc");
  if (!canvas) return null;
  const ctx = canvas.getContext("2d");
  const stateEl = $("#state");
  const stateWord = $("#state-word");
  const rateLine = $("#rate-line");
  const rateNote = $("#rate-note");
  const keys = { stop: $("#k-stop"), play: $("#k-play"), pause: $("#k-pause") };
  const START = [1, 0, 0, 0];
  const COLS = 8 * 5 + 3 + 10; // eight digits, three colons, one blank column between glyphs
  const css = getComputedStyle(document.documentElement);
  const lit = css.getPropertyValue("--phosphor").trim();
  const ghost = css.getPropertyValue("--ghost").trim();

  let rate = RATES["29.97"];
  let state = "stopped";
  let elapsed = 0; // seconds since START at the real rate
  let last = 0;
  let raf = 0;
  let shown = "";
  let pitch = 0;

  const startFrames = () => toFrames(...START, rate);
  const position = () => toTimecode(startFrames() + Math.floor(elapsed * rate.real + 1e-9), rate);
  const pad = (n) => String(n).padStart(2, "0");

  function fit() {
    const dpr = window.devicePixelRatio || 1;
    const inner = canvas.parentElement.clientWidth - 32; // padding-box width minus 2 x 16px padding
    pitch = Math.max(3, Math.floor((inner * dpr) / COLS)); // whole device pixels per cell
    canvas.width = pitch * COLS;
    canvas.height = pitch * 7;
    canvas.style.width = `${canvas.width / dpr}px`;
    canvas.style.height = `${canvas.height / dpr}px`;
    // Centre on a whole CSS pixel and move the graticule with it, so dots stay on the grid cells.
    const offset = Math.max(0, Math.floor((inner - canvas.width / dpr) / 2));
    canvas.style.marginLeft = `${offset}px`;
    canvas.parentElement.style.setProperty("--cell", `${pitch / dpr}px`);
    canvas.parentElement.style.setProperty("--ox", `${offset}px`);
    shown = "";
    draw();
  }

  function draw() {
    const tc = position();
    const text = tc.map(pad).join(":");
    if (text === shown) return;
    shown = text;
    const gap = Math.max(1, Math.round(pitch * 0.2));
    const size = pitch - gap;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    let col = 0;
    for (const ch of text) {
      const glyph = GLYPHS[ch];
      for (let r = 0; r < 7; r++) {
        for (let c = 0; c < glyph[r].length; c++) {
          ctx.fillStyle = glyph[r][c] === "1" ? lit : ghost;
          ctx.fillRect((col + c) * pitch, r * pitch, size, size);
        }
      }
      col += glyph[0].length + 1;
    }
  }

  function tick(now) {
    elapsed += Math.min(0.25, (now - last) / 1000);
    last = now;
    draw();
    if (state === "playing") raf = requestAnimationFrame(tick);
  }

  function setState(next) {
    state = next;
    cancelAnimationFrame(raf);
    if (next === "stopped") elapsed = 0;
    if (next === "playing") {
      last = performance.now();
      raf = requestAnimationFrame(tick);
    }
    refreshText();
    shown = "";
    draw();
  }

  function refreshText() {
    stateEl.dataset.state = state;
    stateWord.textContent = t(`w_${state}`);
    rateLine.textContent = rate.label;
    rateNote.textContent = t(`rn_${rate === RATES["29.97"] ? "29.97" : rate.fps}`);
    for (const [name, key] of Object.entries(keys)) key.setAttribute("aria-pressed", String(state === name));
  }

  keys.stop.addEventListener("click", () => setState("stopped"));
  keys.play.addEventListener("click", () => setState("playing"));
  keys.pause.addEventListener("click", () => { if (state === "playing") setState("paused"); });

  for (const input of $$('input[name="rate"]')) {
    input.addEventListener("change", () => {
      if (!input.checked) return;
      rate = RATES[input.value];
      refreshText();
      shown = "";
      draw();
    });
  }

  $("#jump").addEventListener("click", () => {
    const target = toFrames(1, 0, 58, 0, rate) - startFrames();
    elapsed = target / rate.real;
    if (state === "stopped") setState("paused");
    else { shown = ""; draw(); }
  });

  new ResizeObserver(fit).observe(canvas.parentElement);
  fit();
  refreshText();
  if (!matchMedia("(prefers-reduced-motion: reduce)").matches) setState("playing");
  return { refreshText };
})();

// ---------------------------------------------------------------- start
for (const b of $$(".lang button")) b.addEventListener("click", () => setLang(b.dataset.lang, true));
const fromBrowser = (navigator.language || "").toLowerCase().startsWith("es") ? "es" : "en";
setLang(saved.get() || fromBrowser, false);
