import { invoke } from "@tauri-apps/api/core";
import type {
  Project,
  VjClockSource,
  ProjectState,
  VideoFit,
  VjBlend,
  VjEffectKind,
  VjEffectName,
  VjAudioBand,
  VjLfoShape,
  VjModSource,
  VjGenerator,
  VjModulation,
  VjDirection,
  VjPlayback,
  VjRegion,
  VjTempo,
  VjSide,
  VjTransition,
  VjTrigger,
} from "../project/types";

// Mirrors `VjOp` in src-tauri/crates/flux-project/src/vj.rs and src-tauri/src/vj_commands.rs.

/** An edit of the grid; the backend applies it and answers with the project. */
export type VjOp =
  | { op: "addLayer"; global: boolean }
  | { op: "removeLayer"; layer: string }
  | { op: "moveLayer"; layer: string; to: number }
  | { op: "renameLayer"; layer: string; name: string }
  | { op: "layerMaster" | "layerOpacity" | "layerVolume" | "layerTransition"; layer: string; value: number }
  | { op: "layerBlend"; layer: string; blend: VjBlend }
  | { op: "layerTransitionEffect"; layer: string; effect: VjTransition }
  | { op: "layerCrossfade"; layer: string; side: VjSide | null }
  | { op: "layerBypass" | "layerSolo" | "layerCollapse"; layer: string; on: boolean }
  | { op: "addDeck" }
  | { op: "removeDeck"; deck: string }
  | { op: "moveDeck"; deck: string; to: number }
  | { op: "renameDeck"; deck: string; name: string }
  | { op: "selectDeck"; deck: string }
  | { op: "addColumn" }
  | { op: "removeColumn"; column: string }
  | { op: "moveColumn"; column: string; to: number }
  | { op: "renameColumn"; column: string; name: string }
  | { op: "placeVideo"; asset: string; layer: string; column: string }
  | { op: "placeGenerator"; generator: VjGenerator; layer: string; column: string }
  | { op: "clipGenerator"; clip: string; generator: VjGenerator }
  | { op: "moveClip"; clip: string; layer: string; column: string }
  | { op: "removeClip"; clip: string }
  | { op: "setBpm"; bpm: number }
  | { op: "nudgeBpm"; by: number }
  | { op: "scaleBpm"; factor: number }
  | { op: "tapBpm"; taps: number[] }
  | { op: "setClock"; source: VjClockSource }
  | { op: "renameClip"; clip: string; name: string }
  | { op: "clipPlayback"; clip: string; playback: VjPlayback }
  | { op: "clipSpeed"; clip: string; value: number }
  | { op: "clipDirection"; clip: string; direction: VjDirection }
  | { op: "clipTempo"; clip: string; tempo: VjTempo }
  | { op: "clipRegion"; clip: string; start: number; end: number }
  /** Sets cue `index` (0 to 7) at `position`, a fraction of the clip's length, or empties it with `null`. */
  | { op: "clipCue"; clip: string; index: number; position: number | null }
  | { op: "clipTrigger"; clip: string; trigger: VjTrigger }
  | { op: "clipFit"; clip: string; fit: VideoFit }
  | { op: "clipScale"; clip: string; value: number }
  | { op: "clipPosition"; clip: string; x: number; y: number }
  | { op: "addEffect"; target: { clip: string } | { layer: string }; kind: VjEffectName }
  | { op: "removeEffect"; effect: string }
  | { op: "moveEffect"; effect: string; index: number }
  | { op: "effectParams"; effect: string; kind: VjEffectKind }
  | { op: "effectBypass"; effect: string; bypass: boolean }
  | { op: "duplicateEffect"; effect: string }
  | { op: "resetEffect"; effect: string }
  | { op: "setModulation"; target: ModTarget; modulation: VjModulation | null };

/** What a modulation is set on: a numeric setting of an effect, or a field of a layer or a clip. */
export type ModTarget = { effect: string; param: string } | { layer: string; field: string } | { clip: string; field: string };

export const vjEdit = (project: Project, op: VjOp) => invoke<ProjectState>("vj_edit", { project, op });
/** Moves a level or a clip's transform in the backend while the hand moves it, without the project going back and forth. */
export const vjPreview = (op: VjOp) => invoke<void>("vj_preview", { op });

/** Adds the video at `path` to the cell of `layer` in `column`, or to its first empty cell in the deck shown. */
export const vjImport = (project: Project, path: string, layer: string, column: string | null) =>
  invoke<ProjectState>("project_import_vj_video", { project, path, layer, column });

/** A clip playing on a layer, as the backend answers. */
export interface PlayingClip {
  layer: string;
  clip: string;
  /** Since it was launched. */
  seconds: number;
  /** Where the playhead is in the whole clip, 0 to 1; `null` until its length is known, and once a clip played once has ended. */
  position: number | null;
  /** Where it is in its region, 0 to 1. */
  progress: number | null;
  /** Seconds until it reaches the edge of its region it moves towards, at its pace; `null` while paused. */
  remaining: number | null;
  /** The region and cues of the clip, as the backend plays them. */
  region: VjRegion;
  cues: (number | null)[];
  /** Seconds of the clip that play in a second: negative backwards, 0 paused. */
  rate: number;
  /** How long one pass through the region lasts, in seconds; `null` while paused or unknown. */
  passSeconds: number | null;
  /** How much of the clip shows on the output, 0 to 1. */
  level: number;
  /** How far the layer's transition is, 0 to 1, while one runs. */
  transition: number | null;
  /** Whether the layer is fading out to nothing, showing the clip that played. */
  stopping: boolean;
}

export interface GridState {
  playing: PlayingClip[];
  /** The column launched last, while its clips play. */
  column: string | null;
  /** 0 is all side A, 1 all side B. */
  crossfader: number;
}

export const vjLaunch = (clip: string) => invoke<GridState>("vj_launch", { clip });
export const vjRelease = (clip: string) => invoke<GridState>("vj_release", { clip });
export const vjLaunchColumn = (column: string) => invoke<GridState>("vj_launch_column", { column });
/** Stops a layer, or every layer with `null`. */
/** Moves the playhead of the clip playing on `layer` to `position`, a fraction of the clip's length. */
export const vjScrub = (layer: string, position: number) => invoke<GridState>("vj_scrub", { layer, position });
/** Jumps to cue `index` (0 to 7) of a clip, launching the clip there if it is not playing. */
export const vjCue = (clip: string, index: number) => invoke<GridState>("vj_cue", { clip, index });
export const vjStop = (layer: string | null) => invoke<GridState>("vj_stop", { layer });
export const vjCrossfader = (position: number) => invoke<GridState>("vj_crossfader", { position });
export const vjPlayState = () => invoke<GridState>("vj_play_state");
/** What the grid sends to the screens now, as a small picture (see `bindOutput`); empty outside a VJ project. */
export const vjOutputFrame = () => invoke<ArrayBuffer>("vj_output_frame");

/** A setting of an effect, with what it takes; the backend owns the ranges and the defaults. */
export type VjParamSpec =
  | { type: "number"; key: string; min: number; max: number; step: number; default: number; whole: boolean; unit: string; snap: number[]; modulatable: boolean }
  | { type: "choice"; key: string; options: string[]; default: string }
  | { type: "colour"; key: string; default: string }
  | { type: "flag"; key: string; default: boolean };

/** One kind of effect: its category, whether it moves with time, and its settings. */
export interface VjEffectSpec {
  name: string;
  category: string;
  animated: boolean;
  params: VjParamSpec[];
}

/** A layer's or a clip's modulatable field: its range, its default and where a new modulation of it starts. */
export interface VjModField {
  min: number;
  max: number;
  default: number;
  start: [number, number];
}

/** What modulation is made of, as the backend lists it. */
export interface VjModulationCatalog {
  shapes: VjLfoShape[];
  bands: VjAudioBand[];
  maxGain: number;
  maxSmoothing: number;
  maxRate: number;
  /** The lengths of a synced cycle, in beats. */
  beats: number[];
  /** What a new modulation starts as (min and max are set from the setting). */
  start: VjModulation;
  /** What switching to an audio source starts as. */
  audio: VjModSource;
  layer: Record<string, VjModField>;
  clip: Record<string, VjModField>;
}

export interface VjEffectCatalog {
  /** Most effects a stack holds. */
  max: number;
  modulation: VjModulationCatalog;
  /** The groups effects are listed in, in order. */
  categories: string[];
  effects: VjEffectSpec[];
}

export const vjEffectCatalog = () => invoke<VjEffectCatalog>("vj_effect_catalog");

/** The modulated values of the grid now, by effect id and setting, layer id and field, clip id and field. */
export interface ModValues {
  effects: Record<string, Record<string, number>>;
  layers: Record<string, Record<string, number>>;
  clips: Record<string, Record<string, number>>;
}

export const vjModulationValues = () => invoke<ModValues>("vj_modulation_values");

/** How loud the bands of the sound are, 0 to 1, and what the analysis listens to. */
export interface AnalysisLevels {
  low: number;
  mid: number;
  high: number;
  level: number;
  source: "off" | "grid" | "input";
  state: "off" | "listening" | "missing" | "error";
}

export const vjAnalysisLevels = () => invoke<AnalysisLevels>("vj_analysis_levels");
