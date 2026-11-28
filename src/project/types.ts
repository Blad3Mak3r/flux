// Mirrors the Rust model in src-tauri/crates/flux-project/src/model.rs (the `project.json` shape).
// Positions are frame counts at the frame rate of the timeline they are in.

export type FrameRate = "fps24" | "fps25" | "fps2997Drop" | "fps30";

export interface Project {
  setlists?: import("../show/types").Setlist[];
  formatVersion: number;
  name: string;
  meta: ProjectMeta;
  autosave: Autosave;
  voice?: string;
  assets: AudioAsset[];
  /** The video files the video tracks play; absent when there are none. Ids share the namespace of `assets`. */
  videos?: VideoAsset[];
  output: ShowOutputs;
  /** Never empty. The backend decides which one is active; only that one plays. */
  timelines: Timeline[];
  /** The kind of project, chosen when it is created and never changed; absent is `timeline`. */
  mode?: ProjectMode;
  /** The VJ grid; only a VJ project has one. */
  vj?: VjGrid;
}

export type ProjectMode = "timeline" | "vj";

/**
 * The VJ grid (mirrors `flux-project/src/vj.rs`): layers bottom first, the same in every deck; decks
 * of columns, left first, one of them shown. A global layer keeps its clips in slots, the same in
 * every deck.
 */
export interface VjGrid {
  layers: VjLayer[];
  /** The groups of consecutive layers, bottom first; absent is none. */
  groups?: VjGroup[];
  decks: VjDeck[];
  /** Id of the deck shown. */
  deck: string;
  /** Where the timecode clock comes from; absent is `internal`. */
  clock?: VjClockSource;
  /** Drawn on the final mix of the layers before the output mapping, the first applied first. */
  compositionEffects?: VjEffect[];
  /** The composition's effects are skipped; absent is false. */
  compositionBypass?: boolean;
  /** What MIDI controls and OSC addresses drive; absent is none. */
  mappings?: VjMapping[];
  /** The composition dashboard's knobs; one not listed is at 0 with no links. */
  dashboard?: VjKnob[];
  /** The colours the pads of mappings with none of their own get; absent is the controller's preset. */
  lights?: VjLights;
}

/** What a MIDI pad is sent for each state, 0 to 127 (a colour on controllers like the APC40). */
export interface VjLights {
  off: number;
  loaded: number;
  on: number;
}

// Mirrors `flux-project/src/vj_mapping.rs`.

/** What drives a target: a MIDI control (learned on a port) or an OSC address. */
export type VjSource =
  | { type: "midi"; port?: string; channel: number; kind: "cc" | "note" | "pitchBend"; number: number }
  | { type: "osc"; address: string }
  /** A DMX channel of an Art-Net universe (the Port-Address), 1 to 512; `fine` is the channel of the low byte of a 16-bit value. */
  | { type: "artnet"; universe: number; channel: number; fine?: number }
  /** A key of the keyboard by its place (`KeyboardEvent.code`) and the modifiers held. */
  | { type: "key"; code: string; ctrl?: boolean; alt?: boolean; shift?: boolean; meta?: boolean };

/** How a mapping keeps hold of a clip, layer, column, deck or effect: by id, or by the slot it is in. */
export type VjFollow = "id" | "position";

/** What is driven; every id names something of the grid. */
export type VjTarget =
  | { kind: "clipLaunch"; clip: string }
  | { kind: "columnLaunch"; column: string }
  | { kind: "layerStop" | "layerAutopilot" | "layerBypass" | "layerSolo" | "layerMaster" | "layerVolume" | "layerOpacity" | "layerTransition"; layer: string }
  | { kind: "groupBypass" | "groupSolo" | "groupMaster" | "groupOpacity"; group: string }
  | { kind: "deckSelect"; deck: string }
  | { kind: "effectBypass"; effect: string }
  | { kind: "effectParam"; effect: string; param: string }
  | { kind: "clipSpeed"; clip: string }
  | { kind: "generatorNumber"; clip: string; name: string }
  | { kind: "dashboard"; knob: number }
  | { kind: "layerDashboard"; layer: string; knob: number }
  | { kind: "clipDashboard"; clip: string; knob: number }
  | { kind: "deckNext" | "deckPrev" | "stopAll" | "bpmTap" | "bpmResync" | "bpmPlus" | "bpmMinus" | "bpmDouble" | "bpmHalf" | "bpmNudgeUp" | "bpmNudgeDown" | "bpmPause" | "crossfader" | "bpm" };

/** What a mapped button does: a press runs or turns on, flips a switch, or holds while down. */
export type VjButtonMode = "trigger" | "toggle" | "hold";

export interface VjMapping {
  id: string;
  source: VjSource;
  target: VjTarget;
  /** Where a continuous target goes with the source at its minimum and its maximum, in the target's units. */
  min: number;
  max: number;
  invert?: boolean;
  /** Absent is `trigger`. */
  mode?: VjButtonMode;
  /** Absent is `id`. */
  follow?: VjFollow;
  /** A position mapping whose slot is empty: kept, and it drives nothing until something fills the slot. */
  parked?: boolean;
  /** The colours of the pad of a MIDI source; absent uses the grid's, then the controller's. */
  lights?: VjLights;
}

export interface VjKnob {
  /** 0 to 1. */
  value: number;
  /** What turns the knob by itself. */
  modulation?: VjModulation;
  name?: string;
  links?: VjLink[];
}

export interface VjLink {
  target: VjTarget;
  min: number;
  max: number;
}

/** Flux's own transport, or timecode coming in by LTC or MTC. */
export type VjClockSource = "internal" | "ltc" | "mtc";

export interface VjDeck {
  id: string;
  /** Absent is the deck's number. */
  name?: string;
  columns: VjColumn[];
}

export interface VjColumn {
  id: string;
  /** Absent is the column's number. */
  name?: string;
}

export type VjBlend =
  | "alpha" | "add" | "subtract" | "multiply" | "screen" | "lighten" | "darken"
  | "difference" | "exclusion" | "overlay" | "softLight" | "hardLight" | "colorDodge" | "colorBurn"
  | "linearLight" | "vividLight" | "pinLight" | "hue" | "saturation" | "color" | "luminosity"
  | "alphaCut" | "lumaKey" | "lumaKeyInverted" | "lumaIsAlpha" | "differenceInverted" | "mask50" | "rgb" | "displace";
export type VjSide = "a" | "b";
export type VjAutopilot = "off" | "next" | "previous" | "random";

export interface VjLayer {
  id: string;
  /** Absent is the layer's number. */
  name?: string;
  global?: boolean;
  /** 0 to 1; absent is 1. */
  master?: number;
  /** 0 to 1; absent is 1. */
  opacity?: number;
  /** 0 to 1 as the fader stands; absent is 1. */
  volume?: number;
  /** Absent is `alpha`. */
  blend?: VjBlend;
  /** Seconds, 0 to 10; absent is 0, a cut. */
  transition?: number;
  /** Absent is `alpha`. */
  transitionEffect?: VjBlend;
  /** What the layer launches when its clip ends a pass; absent is `off`. */
  autopilot?: VjAutopilot;
  /** The crossfader side; absent is neither. */
  crossfade?: VjSide;
  bypass?: boolean;
  solo?: boolean;
  /** Folded to a thin row on the grid. */
  collapsed?: boolean;
  /** The id of the group the layer is in. */
  group?: string;
  /** Drawn on each of its clips' pictures after the clip's own effects; the first is applied first. */
  effects?: VjEffect[];
  /** What drives `master`, `opacity` or `volume` by itself. */
  modulation?: VjModulations;
  /** The layer's dashboard; a knob not listed is at 0 with no links. */
  dashboard?: VjKnob[];
  /** At most one per column of each deck, or per slot on a global layer. */
  clips?: VjClip[];
}

/** Layers mixed together first and then blended like a layer (Resolume's layer groups). */
export interface VjGroup {
  id: string;
  /** Absent is the group's number. */
  name?: string;
  /** 0 to 1; absent is 1. */
  master?: number;
  /** 0 to 1; absent is 1. */
  opacity?: number;
  /** Absent is `alpha`. */
  blend?: VjBlend;
  bypass?: boolean;
  solo?: boolean;
  /** Folded to its header on the grid. */
  collapsed?: boolean;
  /** Drawn on the mix of the group's layers, the first applied first. */
  effects?: VjEffect[];
}

export type VjMirrorAxis = "horizontal" | "vertical" | "both";
/** A setting of an effect, as the backend keeps it: a number, a choice or a `#rrggbb` colour, or on or off. */
export type VjParamValue = number | string | boolean;
/**
 * What an effect does, with its settings. Which kinds there are and what each setting takes come from the backend's
 * catalogue (`vjEffectCatalog`), so this only names the kind and carries the settings.
 */
export type VjEffectKind = { effect: string } & { [setting: string]: VjParamValue };
export type VjEffectName = string;
/** One effect of a clip's or a layer's stack. */
export type VjEffect = VjEffectKind & { id: string; bypass?: boolean };
/** What drives the settings of an effect by themselves, by the setting's key (typed apart: the settings are all values). */
export const effectModulation = (effect: VjEffect): VjModulations | undefined =>
  (effect as unknown as { modulation?: VjModulations }).modulation;

/** What an LFO draws and what an audio source follows; the backend's lists are in the effect catalogue. */
export type VjLfoShape = "sine" | "triangle" | "sawUp" | "sawDown" | "square" | "random" | "smoothRandom";
export type VjAudioBand = "low" | "mid" | "high" | "level";
export interface VjPace {
  sync: "free" | "beats";
  /** Cycles a second when free. */
  rate: number;
  /** Beats a cycle when synced. */
  beats: number;
}
export type VjModSource =
  | { kind: "lfo"; shape: VjLfoShape; pace: VjPace; phase: number }
  | { kind: "audio"; band: VjAudioBand; gain: number; attack: number; release: number };
/** A numeric setting driven by itself between `min` and `max`, in the setting's own units (`min` may be above `max`). */
export interface VjModulation {
  source: VjModSource;
  min: number;
  max: number;
}
/** By the key of the setting: an effect's, or `master`, `opacity`, `volume` of a layer, `scale`, `positionX`, `positionY` of a clip. */
export type VjModulations = Record<string, VjModulation>;

/** What a launched clip does at the end of its video; absent is `loop`. */
export type VjPlayback = "loop" | "once" | "pingPong";
/** Which way a clip plays through its region. */
export type VjDirection = "forward" | "reverse" | "pause";
/** Whether one pass of a clip lasts as long as its speed makes it, or a number of beats of the composition's tempo. */
export type VjTempo = { kind: "free" } | { kind: "beats"; beats: number };
/** The part of a clip that plays, as fractions of its length. */
export interface VjRegion {
  start: number;
  end: number;
}
/** What a press on a clip does; absent is `normal`. */
export type VjTrigger = "normal" | "piano";

/** What a generator clip draws, with its parameters; colours are `#rrggbb`. A parameter left out
 * takes the backend's default. */
export type VjGenerator =
  | { kind: "solid"; colour?: string }
  /** `rate` flashes a second, each lasting `width` (0 to 1) of the time between two. */
  | { kind: "strobe"; colour?: string; rate?: number; width?: number }
  /** `angle` in degrees: 0 is left to right, 90 top to bottom. */
  | { kind: "gradient"; from?: string; to?: string; angle?: number }
  | { kind: "bars" }
  /** A camera or capture card, by the name the system gives it; black while there is none or it is not connected. */
  | { kind: "liveInput"; device?: string; size?: LiveSize; rate?: LiveRate }
  /** New snow `rate` times a second. */
  | { kind: "noise"; colour?: string; rate?: number }
  /** A block of text over a clear background. `size` is a fraction of the composition's height, the position the
   * block's centre as fractions of it, `tracking` and `leading` extra space in ems. With `lines` `one`, `line` is the
   * line shown and `advance` steps it every that many beats (0 is by hand). */
  | {
      kind: "text";
      text?: string;
      font?: string;
      style?: string;
      size?: number;
      colour?: string;
      opacity?: number;
      align?: VjTextAlign;
      positionX?: number;
      positionY?: number;
      rotation?: number;
      tracking?: number;
      leading?: number;
      lines?: VjTextLines;
      line?: number;
      advance?: number;
    };
export type VjTextAlign = "left" | "center" | "right";
export type VjTextLines = "all" | "one";
/** A font family the machine has, with the names of its faces. */
export interface VjFont {
  name: string;
  styles: string[];
}
/** The picture size and frame rate a live input asks of its device. */
export type LiveSize = "auto" | "1280x720" | "1920x1080" | "3840x2160";
export type LiveRate = "auto" | "25" | "30" | "50" | "59.94" | "60";
/** What the backend says of a live input's device: `idle` while no clip shows it, `busy` when another program holds it. */
export interface VjCaptureStatus {
  state: "idle" | "notConnected" | "connected" | "busy";
  refused: boolean;
}
/** A camera or capture card: `name` is what a live input clip stores, `label` what to show. */
export interface VjCaptureDevice {
  name: string;
  label: string;
}
export type VjGeneratorKind =VjGenerator["kind"];

export interface VjClip {
  id: string;
  /** On a deck layer: the column it is in. */
  column?: string;
  /** On a global layer: its place from the left. */
  slot?: number;
  /** Id of an entry in `Project.videos`; absent for a generator. */
  asset?: string;
  /** What makes the picture when the clip is not a video. */
  generator?: VjGenerator;
  name?: string;
  playback?: VjPlayback;
  trigger?: VjTrigger;
  /** How fast it plays when its tempo is free; absent is 1. */
  speed?: number;
  /** Seconds a still image or a generator lasts (0.1 to 3600); absent is 10. A video keeps its file's length. */
  duration?: number;
  /** Absent is forward. */
  direction?: VjDirection;
  /** Absent is free. */
  tempo?: VjTempo;
  /** The part that plays and repeats; absent is all of it. */
  region?: VjRegion;
  /** Places the clip jumps to, as fractions of its length: 8, each empty or set; absent are all empty. */
  cues?: (number | null)[];
  fit?: VideoFit;
  /** How much larger than its fit it is drawn, about its centre; absent is 1 (100%). */
  scale?: number;
  /** How far its centre moves from where its fit puts it, as fractions of the composition, right
   * and down; absent is not moved. */
  position?: [number, number];
  /** Drawn on its picture, the first applied first. */
  effects?: VjEffect[];
  /** What drives `scale`, `positionX` or `positionY` by itself. */
  modulation?: VjModulations;
  /** The clip's dashboard, as the layer's. */
  dashboard?: VjKnob[];
}

export interface Autosave {
  enabled: boolean;
  intervalSeconds: 15 | 30 | 60;
}

/** One line of time of a project, e.g. a song, with its own timecode and tracks. */
export interface Timeline {
  controls?: import("../show/types").TimelineControl[];
  id: string;
  name: string;
  timecode: { frameRate: FrameRate; start: number };
  /** The beat grid and the metronome of the timeline. */
  tempo: Tempo;
  tracks: Track[];
}

/** Beats are counted from the timeline origin. Where the click plays is `OutputConfig.metronome`. */
export interface Tempo {
  /** 20 to 400. */
  bpm: number;
  /** 1 to 16. */
  beatsPerBar: number;
  /** Whether the metronome plays while this timeline runs. */
  click: boolean;
}

export const MIN_BPM = 20;
export const MAX_BPM = 400;
export const MAX_BEATS_PER_BAR = 16;
export const DEFAULT_TEMPO: Tempo = { bpm: 120, beatsPerBar: 4, click: false };

/** A project with the id of its active timeline, as the backend answers after it changed something. */
export interface ProjectState {
  project: Project;
  active: string;
}

/** Identity and history of the project; `modifiedAt` and `savedWith` are set by the backend on save. */
export interface ProjectMeta {
  /** A UUID that stays with the project through renames and copies of the file. */
  id: string;
  /** RFC 3339 UTC. */
  createdAt: string;
  modifiedAt: string;
  /** FLUX version that created it; empty when unknown. */
  createdWith: string;
  /** FLUX version of the last save. */
  savedWith: string;
  author?: string;
  description?: string;
}

/** A media file kept outside the project file, found by path. */
export interface MediaLink {
  /** Absolute, as the system gave it. */
  path: string;
  /** Relative to the project file's folder, tried when `path` is gone. */
  relative?: string;
}

export interface AudioAsset {
  id: string;
  name: string;
  /** Archive path, `audio/<file name>`: the asset's stable key, and where its bytes are with no `link`. */
  file: string;
  /** Where the audio lives outside the project file; absent when the project holds it. */
  link?: MediaLink;
  /** The tempo found in the audio or set by the user; absent when it was never detected. */
  bpm?: number;
}

export interface VideoAsset {
  id: string;
  name: string;
  /** Archive path, `video/<file name>`. */
  file: string;
  /** Where the file lives outside the project file; absent when the project holds it. */
  link?: MediaLink;
}

export type AudioAssignment = { type: "inherit" } | { type: "silent" } | { type: "output"; id: string };
export interface AudioBus { id: string; name: string }

export interface Track {
  audioOutput?: AudioAssignment;
  id: string;
  name: string;
  muted: boolean;
  /** The track's fader in dB, audio tracks only. Absent is 0 dB; -60 or less is silence. */
  gainDb?: number;
  /** The colour the track is drawn in, `#rrggbb`. Absent is the default for its kind. */
  color?: string;
  kind: TrackKind;
}

export interface GeneratedVoice {
  origin?: "generated" | "imported" | "recorded";
  synthesisText?: string; asset: string; dialogue: string; voice: string; model: string; samples: number }
export interface VoiceOrder {
  mode?: "generated" | "manual";
  synthesisText?: string;
  trimStart?: number;
  trimEnd?: number; id: string; at: number; dialogue: string; alignment: "start" | "end"; generated?: GeneratedVoice }
export interface VoiceView {
  audioOffsetSeconds: number;
  trimStartMs: string;
  trimEndMs: string; id: string; voice: string | null; source: "track" | "project" | "flux"; status: "ungenerated" | "synced" | "stale"; beforeOrigin: boolean; start: number | null; length: number | null }

export type TrackKind =
  | { type: "operator"; voice?: string; orders: VoiceOrder[] }
  | { type: "audio"; clips: AudioClip[] }
  | { type: "notes"; notes: Note[] }
  | { type: "midi"; port?: MidiPortRef; events: MidiEvent[] }
  | { type: "osc"; destination?: string; events: OscEvent[] }
  | { type: "video"; clips: VideoClip[] };

/** The MIDI port a track sends to, stored as a name hint like the MTC ports. */
export interface MidiPortRef {
  portName: string;
  /** Which of the ports with this name it is, counting from 0 in the system's order; 0 when absent. */
  portIndex?: number;
}

/** A message sent at a point of the timeline. Channels are 1 to 16, other numbers 0 to 127. */
export interface MidiEvent {
  firstTurnOnly?: boolean;
  id: string;
  at: number;
  message: MidiMessage;
}

export type MscCommand = "go" | "stop" | "resume";

export type MidiMessage =
  | { type: "noteOn"; channel: number; note: number; velocity: number; length?: number }
  | { type: "noteOff"; channel: number; note: number }
  | { type: "controlChange"; channel: number; controller: number; value: number }
  | { type: "programChange"; channel: number; program: number }
  | { type: "msc"; deviceId: number; format: number; command: MscCommand; cue?: string; list?: string }
  | { type: "sysEx"; data: number[] };

export type OscArg =
  | { type: "int"; value: number }
  | { type: "float"; value: number }
  | { type: "string"; value: string }
  | { type: "bool"; value: boolean };

export interface OscEvent {
  firstTurnOnly?: boolean;
  id: string;
  at: number;
  /** Starts with `/`. */
  address: string;
  args?: OscArg[];
}

/** An OSC receiver (UDP), defined once for the project and chosen by the tracks that send to it. */
export interface OscDestination {
  id: string;
  name: string;
  host: string;
  port: number;
}

/** How the gain moves from one envelope point to the next. */
export type EnvCurve = "linear" | "smooth" | "fast" | "slow" | "hold";

/** One breakpoint of a clip's volume envelope. */
export interface EnvPoint {
  /** Thousandths of a frame from the start of the clip's audio file, so moving or trimming the clip leaves it be. */
  at: number;
  /** -60 (silence) to +6. */
  gainDb: number;
  /** How the gain gets from this point to the next; absent is a straight line. */
  curve?: EnvCurve;
}

/** Envelope points per frame. */
export const ENVELOPE_TICKS_PER_FRAME = 1000;
export const ENVELOPE_MIN_DB = -60;
export const ENVELOPE_MAX_DB = 6;
export const MAX_ENVELOPE_POINTS = 256;

/** How the gain of a fade runs between silence and full level. */
export type FadeCurve = "linear" | "equalPower" | "sCurve" | "exponential";

export interface AudioClip {
  id: string;
  /** Id of an entry in `Project.assets`. */
  asset: string;
  start: number;
  gainDb: number;
  /** The name the clip goes by in the timeline and in Live; absent means the asset's name. */
  name?: string;
  /** Frames of the asset skipped at the start of the clip (trim in); absent is 0. */
  offset?: number;
  /** Frames of the asset the clip plays (trim out); absent means up to the end of the asset. */
  length?: number;
  /** Frames the clip takes to rise from silence; absent is 0. */
  fadeIn?: number;
  /** Frames the clip takes to fall to silence; absent is 0. */
  fadeOut?: number;
  /** Shape of the fade in; absent is a straight line. */
  fadeInCurve?: FadeCurve;
  /** Shape of the fade out; absent is a straight line. */
  fadeOutCurve?: FadeCurve;
  /** The volume drawn over the clip; absent is none. */
  envelope?: EnvPoint[];
}

/** A stretch of a video file on the timeline. Positions are frames at the timeline's rate. */
export interface VideoClip {
  id: string;
  /** Id of an entry in `Project.videos`. */
  asset: string;
  start: number;
  /** The name the clip goes by in the timeline and in Live; absent means the video's name. */
  name?: string;
  /** Frames of the video skipped at the start of the clip (trim in); absent is 0. */
  offset?: number;
  /** Frames of the video the clip shows (trim out); absent means up to the end of the video. */
  length?: number;
  /** How the picture is sized into the composition; absent is `fit`. */
  fit?: VideoFit;
  /** Frames the picture takes to rise from black (or from what is below); absent is 0. */
  fadeIn?: number;
  /** Frames the picture takes to fall to black (or to what is below) at its end; absent is 0. */
  fadeOut?: number;
}

/** How a video clip is sized into the composition, as the backend names it. */
export type VideoFit = "fit" | "fill" | "stretch" | "none";
export const VIDEO_FITS: readonly VideoFit[] = ["fit", "fill", "stretch", "none"];

/**
 * Text for the operator at a point of the timeline. It is shown from `at` for `length` frames, or
 * until the next note of its track when it has no length.
 */
export interface Note {
  id: string;
  at: number;
  /** Frames the note lasts; absent means until the next note. */
  length?: number;
  /** Free text, possibly several lines. */
  text: string;
}

/** Portable signals; physical routing lives in this installation's preferences. */
export interface ShowOutputs {
  audio?: AudioBus[];
  metronome?: { gainDb: number } | null;
  ltc: { slot: number; level?: number }[];
  mtc: { slot: number }[];
  osc?: OscDestination[];
  legacyRoutes?: OutputConfig;
  /** The screens of the video output and their slices; none is the main screen with no slices. */
  screens?: import("../video/mapping").VideoScreen[];
  /** The size of the picture the clips are sized into; absent is the first video's size. */
  composition?: { width: number; height: number };
}

/** Devices are stored by name hint only, never by OS identifier. */
export interface OutputConfig {
  audioRoutes?: { id: string; route: OutputConfig["audio"] }[];
  trackRoutes?: { trackId: string; output: string | null }[];
  operator?: OutputConfig["audio"];
  /** The audio driver (API) the audio devices below belong to, e.g. "PipeWire". Absent: the system's default one. */
  driver?: string;
  /** Where the project's audio clips play. `channels` are 1-based, e.g. `[1, 2]`. */
  audio: { deviceName: string; channels: number[] } | null;
  /** Where the Edit view auditions the clips instead of `audio`, with no timecode. Absent: Edit plays like Live. */
  preview?: { deviceName: string; channels: number[] };
  /** Where the metronome clicks, with a device and level of its own. Absent: no click plays. */
  metronome?: MetronomeOutputConfig;
  /** Any number of LTC signals, each on its own device and 1-based channel. */
  ltc: LtcOutputConfig[];
  /** Any number of MIDI ports that receive MTC. */
  mtc: MtcOutputConfig[];
  /** The OSC receivers the OSC tracks send to. Absent: none. */
  osc?: OscDestination[];
}

export interface MetronomeOutputConfig {
  deviceName: string;
  /** 1-based; a pair plays the click on both. */
  channels: number[];
  /** Level of the click in dB; 0 when absent. */
  gainDb?: number;
}

export interface LtcOutputConfig {
  deviceName: string;
  /** 1-based. */
  channel: number;
  /** Peak level of the signal, linear from 0 to 1; 0.5 when absent. */
  level?: number;
  /** Latency offset in ms, positive = emitted earlier; 0 when absent. The backend sets it. */
  offsetMs?: number;
}

export interface MtcOutputConfig {
  portName: string;
  /** Which of the ports with this name it is, counting from 0 in the system's order; 0 when absent. */
  portIndex?: number;
  /** Latency offset in ms, positive = emitted earlier; 0 when absent. The backend sets it. */
  offsetMs?: number;
}
