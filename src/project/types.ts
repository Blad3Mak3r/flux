// Mirrors the Rust model in src-tauri/crates/flux-project/src/model.rs (the `project.json` shape).
// Positions are frame counts at the frame rate of the timeline they are in.

export type FrameRate = "fps24" | "fps25" | "fps2997Drop" | "fps30";

export interface Project {
  formatVersion: number;
  name: string;
  meta: ProjectMeta;
  assets: AudioAsset[];
  output: OutputConfig;
  /** Never empty. The backend decides which one is active; only that one plays. */
  timelines: Timeline[];
}

/** One line of time of a project, e.g. a song, with its own timecode and tracks. */
export interface Timeline {
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
  /** FluxTimecode version that created it; empty when unknown. */
  createdWith: string;
  /** FluxTimecode version of the last save. */
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

export interface Track {
  id: string;
  name: string;
  muted: boolean;
  /** The track's fader in dB, audio tracks only. Absent is 0 dB; -60 or less is silence. */
  gainDb?: number;
  /** The colour the track is drawn in, `#rrggbb`. Absent is the default for its kind. */
  color?: string;
  kind: TrackKind;
}

export type TrackKind =
  | { type: "audio"; clips: AudioClip[] }
  | { type: "notes"; notes: Note[] }
  | { type: "midi"; port?: MidiPortRef; events: MidiEvent[] }
  | { type: "osc"; destination?: string; events: OscEvent[] };

/** The MIDI port a track sends to, stored as a name hint like the MTC ports. */
export interface MidiPortRef {
  portName: string;
  /** Which of the ports with this name it is, counting from 0 in the system's order; 0 when absent. */
  portIndex?: number;
}

/** A message sent at a point of the timeline. Channels are 1 to 16, other numbers 0 to 127. */
export interface MidiEvent {
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

export interface AudioClip {
  id: string;
  /** Id of an entry in `Project.assets`. */
  asset: string;
  start: number;
  gainDb: number;
  /** Frames of the asset skipped at the start of the clip (trim in); absent is 0. */
  offset?: number;
  /** Frames of the asset the clip plays (trim out); absent means up to the end of the asset. */
  length?: number;
  /** Frames the clip takes to rise from silence; absent is 0. */
  fadeIn?: number;
  /** Frames the clip takes to fall to silence; absent is 0. */
  fadeOut?: number;
}

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

/** Devices are stored by name hint only, never by OS identifier. */
export interface OutputConfig {
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
