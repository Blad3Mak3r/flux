// Mirrors the Rust model in src-tauri/src/project/model.rs (the `project.json` shape).
// Positions are frame counts at the project frame rate.

export type FrameRate = "fps24" | "fps25" | "fps2997Drop" | "fps30";

export interface Project {
  formatVersion: number;
  name: string;
  timecode: { frameRate: FrameRate; start: number };
  assets: AudioAsset[];
  tracks: Track[];
  output: OutputConfig;
}

export interface AudioAsset {
  id: string;
  name: string;
  /** Archive path, `audio/<file name>`. */
  file: string;
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
  | { type: "events"; triggers: Trigger[] };

export interface AudioClip {
  id: string;
  /** Id of an entry in `Project.assets`. */
  asset: string;
  start: number;
  gainDb: number;
}

export interface Trigger {
  id: string;
  at: number;
  label: string;
  action: TriggerAction;
}

export type TriggerAction = { type: "marker" } | { type: "midi"; bytes: number[] };

/** Devices are stored by name hint only, never by OS identifier. */
export interface OutputConfig {
  /** Where the project's audio clips play. `channels` are 1-based, e.g. `[1, 2]`. */
  audio: { deviceName: string; channels: number[] } | null;
  /** `channel` is 1-based. */
  ltc: { deviceName: string; channel: number } | null;
  mtc: { portName: string } | null;
}
