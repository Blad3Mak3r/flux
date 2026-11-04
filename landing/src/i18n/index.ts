// English is the default and the source of the keys; Spanish ships beside it. No string lives in
// a component.

export const LOCALES = ["en", "es"] as const;
export type Locale = (typeof LOCALES)[number];

const en = {
  "meta.title": "Flux Timecode: timecode tracks for live shows",
  "meta.description":
    "Build timecode tracks from songs, events and cues on one timeline, and play them out as LTC and MTC. Download Flux Timecode for Windows, macOS and Linux.",
  "brand": "Flux Timecode",
  "nav.label": "Main",
  "nav.downloads": "Downloads",
  "nav.language": "Language",

  "hero.title": "Timecode tracks for live shows",
  "hero.lead":
    "Lay songs, events and cues on one timeline. Flux plays them out as LTC through your audio interface and as MTC through a MIDI port, so lights, video and sound stay locked to the same clock.",
  "hero.download": "Download",
  "hero.downloadFor.windows": "Download for Windows",
  "hero.downloadFor.macos": "Download for macOS",
  "hero.downloadFor.linux": "Download for Linux",
  "hero.other": "Other systems",
  "hero.soon": "First release coming soon",
  "hero.soonNote": "The installers appear here when the first version is published.",
  "hero.early": "Flux is under active development. Expect changes before 1.0.",

  "readout.label": "Timecode readout, for illustration",
  "timeline.note": "Illustration of the timeline, not a screenshot.",
  "timeline.track": "Song",
  "timeline.marker": "Marker",
  "timeline.midi": "MIDI",

  "spec.timeline.title": "Timeline",
  "spec.timeline.body":
    "Songs, events and cues sit on a REAPER-style timeline: tracks on the left, a ruler read in frames, an inspector on the right.",
  "spec.cues.title": "Cues",
  "spec.cues.body":
    "Show control the way QLab lays it out. Markers and MIDI cues go exactly where they happen in the show.",
  "spec.project.title": "One file",
  "spec.project.body":
    "A show is a single .fxtc file: the project and its audio, zipped. It opens on another machine and just works.",

  "clock.title": "Flux is the clock",
  "clock.body":
    "Flux generates timecode and does not chase an outside source. The audio device's sample clock drives playback, LTC and MTC, so the time on screen is the time you send.",
  "clock.ltc": "LTC",
  "clock.ltc.status": "Audio",
  "clock.ltc.detail": "Through your audio interface",
  "clock.mtc": "MTC",
  "clock.mtc.status": "MIDI",
  "clock.mtc.detail": "Through a MIDI port",
  "clock.rate": "Rate",
  "clock.rate.status": "fps",
  "clock.rate.detail": "24, 25, 29.97 drop-frame, 30",

  "dl.title": "Downloads",
  "dl.version": "Version {version}, published {date}",
  "dl.system": "System",
  "dl.format": "Format",
  "dl.size": "Size",
  "dl.checksum": "SHA-256",
  "dl.get": "Download",
  "dl.yours": "Your system",
  "dl.windows": "Windows, 64-bit",
  "dl.macos.aarch64": "macOS, Apple silicon",
  "dl.macos.x86_64": "macOS, Intel",
  "dl.linux.deb": "Linux, Debian and Ubuntu",
  "dl.linux.rpm": "Linux, Fedora and openSUSE",
  "dl.other": "Other",

  "first.title": "First launch",
  "first.windows.name": "Windows",
  "first.windows.body":
    "Windows may warn about an unknown publisher. Choose More info, then Run anyway. The installer asks for administrator permission because it installs for every user.",
  "first.macos.name": "macOS",
  "first.macos.body":
    "Open the DMG and drag Flux to Applications. The first time, right-click it and choose Open.",
  "first.linux.name": "Linux",
  "first.linux.body":
    "Install the package with your package manager, for example sudo apt install ./file.deb or sudo dnf install ./file.rpm.",
  "first.updates":
    "Flux looks for updates each time it starts and installs them before it opens. On Windows and Linux, installing asks for your permission.",

  "footer.version": "Flux Timecode {version}",
  "footer.source": "Source on GitHub",
  "footer.feed": "Update feed",
} as const;

export type Key = keyof typeof en;

const es: Record<Key, string> = {
  "meta.title": "Flux Timecode: pistas de timecode para shows en directo",
  "meta.description":
    "Construye pistas de timecode con canciones, eventos y cues en una sola línea de tiempo y reprodúcelas como LTC y MTC. Descarga Flux Timecode para Windows, macOS y Linux.",
  "brand": "Flux Timecode",
  "nav.label": "Principal",
  "nav.downloads": "Descargas",
  "nav.language": "Idioma",

  "hero.title": "Pistas de timecode para shows en directo",
  "hero.lead":
    "Coloca canciones, eventos y cues en una sola línea de tiempo. Flux los reproduce como LTC por tu interfaz de audio y como MTC por un puerto MIDI, para que luces, vídeo y sonido sigan el mismo reloj.",
  "hero.download": "Descargar",
  "hero.downloadFor.windows": "Descargar para Windows",
  "hero.downloadFor.macos": "Descargar para macOS",
  "hero.downloadFor.linux": "Descargar para Linux",
  "hero.other": "Otros sistemas",
  "hero.soon": "La primera versión llega pronto",
  "hero.soonNote": "Los instaladores aparecerán aquí cuando se publique la primera versión.",
  "hero.early": "Flux está en pleno desarrollo. Habrá cambios antes de la 1.0.",

  "readout.label": "Lectura de timecode, a modo de ilustración",
  "timeline.note": "Ilustración de la línea de tiempo, no una captura.",
  "timeline.track": "Canción",
  "timeline.marker": "Marcador",
  "timeline.midi": "MIDI",

  "spec.timeline.title": "Línea de tiempo",
  "spec.timeline.body":
    "Canciones, eventos y cues viven en una línea de tiempo al estilo REAPER: pistas a la izquierda, una regla en fotogramas y un inspector a la derecha.",
  "spec.cues.title": "Cues",
  "spec.cues.body":
    "Control de show como lo plantea QLab. Los marcadores y los cues MIDI van justo donde ocurren en el show.",
  "spec.project.title": "Un solo archivo",
  "spec.project.body":
    "Un show es un único archivo .fxtc: el proyecto y su audio, comprimidos. Se abre en otro equipo y funciona.",

  "clock.title": "Flux es el reloj",
  "clock.body":
    "Flux genera timecode y no sigue una fuente externa. El reloj de muestreo del dispositivo de audio gobierna la reproducción, el LTC y el MTC, así que el tiempo en pantalla es el que envías.",
  "clock.ltc": "LTC",
  "clock.ltc.status": "Audio",
  "clock.ltc.detail": "Por tu interfaz de audio",
  "clock.mtc": "MTC",
  "clock.mtc.status": "MIDI",
  "clock.mtc.detail": "Por un puerto MIDI",
  "clock.rate": "Tasa",
  "clock.rate.status": "fps",
  "clock.rate.detail": "24, 25, 29,97 con salto de cuadro, 30",

  "dl.title": "Descargas",
  "dl.version": "Versión {version}, publicada el {date}",
  "dl.system": "Sistema",
  "dl.format": "Formato",
  "dl.size": "Tamaño",
  "dl.checksum": "SHA-256",
  "dl.get": "Descargar",
  "dl.yours": "Tu sistema",
  "dl.windows": "Windows, 64 bits",
  "dl.macos.aarch64": "macOS, Apple silicon",
  "dl.macos.x86_64": "macOS, Intel",
  "dl.linux.deb": "Linux, Debian y Ubuntu",
  "dl.linux.rpm": "Linux, Fedora y openSUSE",
  "dl.other": "Otro",

  "first.title": "Primer inicio",
  "first.windows.name": "Windows",
  "first.windows.body":
    "Windows puede avisar de un editor desconocido. Elige Más información y después Ejecutar de todas formas. El instalador pide permiso de administrador porque instala para todos los usuarios.",
  "first.macos.name": "macOS",
  "first.macos.body":
    "Abre el DMG y arrastra Flux a Aplicaciones. La primera vez, haz clic derecho sobre la app y elige Abrir.",
  "first.linux.name": "Linux",
  "first.linux.body":
    "Instala el paquete con tu gestor de paquetes, por ejemplo sudo apt install ./archivo.deb o sudo dnf install ./archivo.rpm.",
  "first.updates":
    "Flux busca actualizaciones cada vez que arranca y las instala antes de abrirse. En Windows y Linux, instalar pide tu permiso.",

  "footer.version": "Flux Timecode {version}",
  "footer.source": "Código en GitHub",
  "footer.feed": "Canal de actualizaciones",
};

const MESSAGES: Record<Locale, Record<Key, string>> = { en, es };

export type Translate = (key: Key, values?: Record<string, string | number>) => string;

export function translator(locale: Locale): Translate {
  return (key, values) => {
    const message = MESSAGES[locale][key];
    if (!values) return message;
    return message.replace(/\{(\w+)\}/g, (whole, name: string) => (name in values ? String(values[name]) : whole));
  };
}

/** The page of `locale` under the site's base path (`/flux-timecode/`). */
export function pagePath(locale: Locale, base: string): string {
  const root = base.endsWith("/") ? base : `${base}/`;
  return locale === "en" ? root : `${root}${locale}/`;
}
