// FLUX landing: language selector, section index, and the Fig. 1 demo clock.
// English lives in the HTML (it reads fine without JS); Spanish is swapped in from ES below.
"use strict";

const ES = {
  title: "FLUX: construye el show, ejecuta el show, confía en el reloj",
  desc: "FLUX es una app de escritorio que convierte canciones, eventos y cues en un único proyecto con un reloj maestro de timecode: salida LTC y MTC, cues MIDI y OSC, y una vista Live para la cabina.",
  skip: "Saltar al contenido", rev: "Hoja de datos, rev.", docs: "Documentación", download: "Descargar",
  lang_label: "Idioma", toc_label: "Secciones",
  toc_general: "General", toc_outputs: "Salidas", toc_edit: "Edición", toc_live: "Directo", toc_setlists: "Setlists",
  toc_remote: "Control remoto", toc_project: "Archivo de proyecto", toc_specs: "Especificaciones", toc_order: "Descarga",
  h1a: "Construye el show.", h1b: "Ejecuta el show.", h1c: "Confía en el reloj.",
  lead: "Una app de escritorio que convierte canciones, eventos y cues en un único proyecto con un reloj maestro de timecode: el modelo de cues de QLab sobre una línea de tiempo al estilo REAPER.",
  o_version: "Versión", o_platforms: "Plataformas", o_platforms_v: "Windows, Linux", o_format: "Archivo de proyecto",
  download_flux: "Descargar FLUX", o_note: "Abre la última versión en GitHub. Desarrollo temprano; todavía no hay versión para macOS.",
  start_line: "INICIO 01:00:00:00", k_stop: "Stop", k_play: "Play", k_pause: "Pausa",
  rate_legend: "Velocidad de fotogramas", jump: "Ir a 01:00:58:00",
  fig1: "Un reloj de demostración sintético que corre en esta página. No es la app y nada sale de tu navegador.",
  tc_label: "Visor de timecode de demostración", transport_label: "Transporte",
  who_label: "Para quién es FLUX",
  why: "Las herramientas basadas en cues ejecutan cues sin una línea de tiempo real. Las de línea de tiempo te dan la línea de tiempo sin control del show. FLUX junta ambas cosas, y el timecode sale del mismo proyecto que guarda el audio y los cues, así que lo que ves en pantalla y lo que sale hacia el equipo no pueden discrepar.",
  w1: "Técnicos de show", w1d: "Operadores de luces, vídeo y sonido que sincronizan equipos con timecode, en una cabina oscura, bajo presión, leyendo la pantalla desde el otro lado de la sala.",
  w2: "Músicos", w2d: "Intérpretes con pistas de acompañamiento que necesitan el audio y los cues del show bloqueados entre sí.",
  w3: "Diseñadores de show", w3d: "Quienes preparan el show con antelación en el estudio, con espacio para pensar.",
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
  fig2: "Camino de la señal. La línea discontinua mueve el reloj; nunca lo alimenta.",
  h_edit: "Constrúyelo en una línea de tiempo",
  tl_label: "Ilustración dibujada de la vista Edit: una regla sobre cuatro pistas, un clip de audio con su forma de onda, una nota, cues MIDI y cues OSC, y un cabezal de reproducción.",
  fig3: "La vista Edit, dibujada para esta página con contenido inventado. No es una captura. El audio es una forma de onda sólida, una nota es un recuadro rayado, MIDI es un cuadrado sólido y OSC uno discontinuo.",
  e1_t: "Líneas de tiempo", e1: "Varias por proyecto, cada una con sus pistas, su velocidad de fotogramas y su desfase de inicio.",
  e2_t: "Pistas", e2: "Audio, notas, MIDI y OSC.",
  e3_t: "Audio", e3: "Importa wav, aiff, flac, mp3 y ogg, mezclados con el reloj del dispositivo de audio, con detección de tempo y clic de metrónomo.",
  e4_t: "Voz del operador", e4: "Escribe diálogos y asocia voz generada (ElevenLabs, con tu propia clave de API) o audio importado y grabado. La voz generada se archiva en el proyecto, así que suena sin conexión.",
  e5_t: "Enrutado", e5: "Envía cada pista a un destino con nombre o a Silencio. Cada ordenador asigna los destinos a sus propias interfaces y canales.",
  h_live: "Ejecútalo desde la cabina",
  live_p: "Pasa de Edit a Live con un solo control (Ctrl+1, Ctrl+2). Live es de solo lectura salvo para ir a un cue, y está hecho para leerse desde el otro lado de la sala.",
  live_alt: "Vista Live de FLUX: un timecode grande con el transporte arriba, el cue en marcha con su cuenta atrás a la izquierda y la lista de cues a la derecha",
  fig4: "La vista Live, con datos de un show de ejemplo. Esta sí es una captura real de la app.",
  l1_t: "El tiempo primero", l1: "Un timecode que se lee de un vistazo, con el estado del transporte escrito en palabras y no solo en color.",
  l2_t: "El cue en marcha", l2: "El cue que suena con su cuenta atrás, lo que está en espera y las notas del operador.",
  l3_t: "Toda la lista de cues", l3: "El backend deriva los cues y su estado, así que la pantalla solo dibuja lo que sabe el motor.",
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
  h_project: "Un show es un solo archivo",
  project_p: 'Un proyecto <span class="m">.flx</span> es un zip con <span class="m">project.json</span> y el audio que usan las pistas, así que un show se abre en otro equipo y simplemente funciona. Los proyectos <span class="m">.fxtc</span> antiguos siguen abriéndose.',
  tree_label: "Contenido de un archivo .flx", tree1: "líneas de tiempo, pistas, salidas", tree2: "los archivos que reproducen las pistas",
  fig5: "Estructura de un archivo de proyecto.",
  h_specs: "Especificaciones y estado",
  sp4_t: "Audio importable",
  sp5_t: "Plataformas", sp5: "Windows (instalador), Linux (AppImage, DEB, RPM).",
  sp6_t: "Idiomas", sp6: "Inglés y español.",
  sp7_t: "Diagnóstico", sp7: "Un registro rotativo y un zip con un clic con los registros y un informe del sistema y los dispositivos, desde el menú Ayuda.",
  st_on: "Disponible hoy",
  st1: "Proyectos .flx con varias líneas de tiempo", st2: "Pistas de audio y notas, detección de tempo, metrónomo",
  st3: "Generación de LTC y MTC, con un transporte completo", st4: "Pistas de cues MIDI y OSC", st5: "Vista Live y setlists con GO",
  st6: "Pistas de voz del operador", st7: "Control remoto MIDI y OSC, con pads iluminados",
  st_off: "Aún no construido",
  sn1: "Cues de vídeo", sn2: "Cues Art-Net", sn3: "Seguir un timecode externo", sn5: "Versiones para macOS",
  h_order: "Consigue FLUX",
  order_p: "FLUX está en desarrollo temprano. Elige el instalador para tu sistema en la página de la última versión: el instalador de Windows, o el AppImage, DEB o RPM de Linux.",
  source: "Ejecútalo desde el código", docs_all: "Lee la documentación", issues: "Informa de un problema",
  foot: "Construye el show. Ejecuta el show. Confía en el reloj.",
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

// ------------------------------------------------------------ section index
(() => {
  const links = new Map($$(".toc a").map((a) => [a.getAttribute("href").slice(1), a]));
  const sections = [...links.keys()].map((id) => document.getElementById(id)).filter(Boolean);
  if (!sections.length || !("IntersectionObserver" in window)) return;
  const set = (id) => { for (const [k, a] of links) k === id ? a.setAttribute("aria-current", "true") : a.removeAttribute("aria-current"); };
  const io = new IntersectionObserver((entries) => {
    for (const e of entries) if (e.isIntersecting) set(e.target.id);
  }, { rootMargin: "-20% 0px -70% 0px" });
  sections.forEach((s) => io.observe(s));
  set("general");
})();

// ---------------------------------------------------------------- start
for (const b of $$(".lang button")) b.addEventListener("click", () => setLang(b.dataset.lang, true));
const fromBrowser = (navigator.language || "").toLowerCase().startsWith("es") ? "es" : "en";
setLang(saved.get() || fromBrowser, false);
