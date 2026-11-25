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
  decks: VjDeck[];
  /** Id of the deck shown. */
  deck: string;
  /** Where the timecode clock comes from; absent is `internal`. */
  clock?: VjClockSource;
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

export type VjBlend = "alpha" | "add" | "subtract" | "multiply" | "screen" | "lighten" | "darken";
export type VjTransition = "alpha" | "add";
export type VjSide = "a" | "b";

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
  transitionEffect?: VjTransition;
  /** The crossfader side; absent is neither. */
  crossfade?: VjSide;
  bypass?: boolean;
  solo?: boolean;
  /** Folded to a thin row on the grid. */
  collapsed?: boolean;
  /** Drawn on each of its clips' pictures after the clip's own effects; the first is applied first. */
  effects?: VjEffect[];
  /** At most one per column of each deck, or per slot on a global layer. */
  clips?: VjClip[];
}

export type VjMirrorAxis = "horizontal" | "vertical" | "both";
/** What an effect does, with its settings as the backend answers them. */
export type VjEffectKind =
  | { effect: "brightness"; amount: number }
  | { effect: "contrast"; amount: number }
  | { effect: "saturation"; amount: number }
  | { effect: "hue"; degrees: number }
  | { effect: "tint"; colour: string; amount: number }
  | { effect: "invert"; amount: number }
  | { effect: "blur"; radius: number }
  | { effect: "mirror"; axis: VjMirrorAxis }
  | { effect: "zoom"; amount: number }
  | { effect: "shift"; x: number; y: number };
export type VjEffectName = VjEffectKind["effect"];
/** One effect of a clip's or a layer's stack. */
export type VjEffect = VjEffectKind & { id: string; bypass?: boolean };

/** What a launched clip does at the end of its video; absent is `loop`. */
export type VjPlayback = "loop" | "once" | "pingPong";
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
  /** New snow `rate` times a second. */
  | { kind: "noise"; colour?: string; rate?: number };
export type VjGeneratorKind = VjGenerator["kind"];

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
  fit?: VideoFit;
  /** How much larger than its fit it is drawn, about its centre; absent is 1 (100%). */
  scale?: number;
  /** How far its centre moves from where its fit puts it, as fractions of the composition, right
   * and down; absent is not moved. */
  position?: [number, number];
  /** Drawn on its picture, the first applied first. */
  effects?: VjEffect[];
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

export interface AudioAsset {
  id: string;
  name: string;
  /** Archive path, `audio/<file name>`. */
  file: string;
  /** The tempo found in the audio or set by the user; absent when it was never detected. */
  bpm?: number;
}

export interface VideoAsset {
  id: string;
  name: string;
  /** Archive path, `video/<file name>`. */
  file: string;
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
