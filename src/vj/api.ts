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
  VjFont,
  VjGenerator,
  VjCaptureDevice,
  VjCaptureStatus,
  VjModulation,
  VjDirection,
  VjPlayback,
  VjRegion,
  VjTempo,
  VjAutopilot,
  VjSide,
  VjTrigger,
  VjButtonMode,
  VjFollow,
  VjSource,
  VjTarget,
  VjLights,
} from "../project/types";

// Mirrors `VjOp` in src-tauri/crates/flux-project/src/vj.rs and src-tauri/src/vj_commands.rs.

/** A fader a layer has; the one `layerLevelReset` brings back to its default. */
export type VjLevel = "master" | "opacity" | "volume" | "transition";

/** An edit of the grid; the backend applies it and answers with the project. */
export type VjOp =
  | { op: "addLayer"; global: boolean }
  | { op: "removeLayer"; layer: string }
  | { op: "moveLayer"; layer: string; to: number }
  | { op: "renameLayer"; layer: string; name: string }
  | { op: "layerMaster" | "layerOpacity" | "layerVolume" | "layerTransition"; layer: string; value: number }
  | { op: "layerLevelReset"; layer: string; level: VjLevel }
  | { op: "layerBlend"; layer: string; blend: VjBlend }
  | { op: "layerTransitionEffect"; layer: string; effect: VjBlend; random?: boolean }
  | { op: "layerAutopilot"; layer: string; mode: VjAutopilot }
  | { op: "layerCrossfade"; layer: string; side: VjSide | null }
  | { op: "layerBypass" | "layerSolo" | "layerCollapse"; layer: string; on: boolean }
  /** Puts the layers in a new group, gathered where the lowest of them is. */
  | { op: "groupLayers"; layers: string[] }
  /** Puts the layer in a group (or none), on its top unless `to` places it in the drawing order. */
  | { op: "layerGroup"; layer: string; group: string | null; to: number | null }
  | { op: "ungroup"; group: string }
  | { op: "renameGroup"; group: string; name: string }
  | { op: "groupMaster" | "groupOpacity"; group: string; value: number }
  | { op: "groupLevelReset"; group: string; level: "master" | "opacity" }
  | { op: "groupBlend"; group: string; blend: VjBlend }
  | { op: "groupBypass" | "groupSolo" | "groupCollapse"; group: string; on: boolean }
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
  /** Moves the line a text block shows by `step` lines, skipping the empty ones and wrapping round. */
  | { op: "textLine"; clip: string; step: number }
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
  /** Seconds a still image or a generator lasts, from 0.1 to 3600. */
  | { op: "clipDuration"; clip: string; value: number }
  | { op: "clipDirection"; clip: string; direction: VjDirection }
  | { op: "clipTempo"; clip: string; tempo: VjTempo }
  | { op: "clipRegion"; clip: string; start: number; end: number }
  /** Sets cue `index` (0 to 7) at `position`, a fraction of the clip's length, or empties it with `null`. */
  | { op: "clipCue"; clip: string; index: number; position: number | null }
  | { op: "clipTrigger"; clip: string; trigger: VjTrigger }
  | { op: "clipFit"; clip: string; fit: VideoFit }
  | { op: "clipScale"; clip: string; value: number }
  | { op: "clipPosition"; clip: string; x: number; y: number }
  | { op: "addEffect"; target: { clip: string } | { layer: string } | { group: string } | "composition"; kind: VjEffectName }
  | { op: "compositionBypass"; bypass: boolean }
  | { op: "removeEffect"; effect: string }
  | { op: "moveEffect"; effect: string; index: number }
  | { op: "effectParams"; effect: string; kind: VjEffectKind }
  | { op: "effectBypass"; effect: string; bypass: boolean }
  | { op: "duplicateEffect"; effect: string }
  | { op: "resetEffect"; effect: string }
  | { op: "setModulation"; target: ModTarget; modulation: VjModulation | null }
  /** Makes the control `source` drive `target`; the latest wins, and a target learned again keeps its options. */
  | { op: "addMapping"; source: VjSource; target: VjTarget }
  | { op: "removeMapping"; mapping: string }
  | { op: "clearMapping"; target: VjTarget }
  /** Pairs a DMX mapping with the channel of its 16-bit low byte, or back to 8 bits with `null`. */
  | { op: "setMappingFine"; mapping: string; fine: number | null }
  /** The values a mapping's pad is sent for its states, or `null` to use the grid's. */
  | { op: "setMappingLights"; mapping: string; lights: VjLights | null }
  /** The colours of the pads whose mapping has none of its own, or `null` for the controller's preset. */
  | { op: "setLights"; lights: VjLights | null }
  | { op: "setOscFeedback"; destinations: string }
  /** Follows a clip, layer, column, deck or effect by id or by its slot. */
  | { op: "setMappingFollow"; mapping: string; follow: VjFollow }
  | { op: "setMapping"; mapping: string; min: number; max: number; invert: boolean; mode: VjButtonMode }
  /** Turns a dashboard knob (0 to 1) and moves what it is linked to. */
  | { op: "dashboardValue"; knob: number; value: number }
  /** Links a number to a knob; a bound left out is the target's own. */
  | { op: "dashboardLink"; knob: number; target: VjTarget; min?: number; max?: number }
  | { op: "dashboardUnlink"; knob: number; target: VjTarget }
  | { op: "renameKnob"; knob: number; name: string };

/** What a modulation is set on: a numeric setting of an effect, or a field of a layer or a clip. */
export type ModTarget = { effect: string; param: string } | { layer: string; field: string } | { clip: string; field: string } | { knob: number };

/** The font families a text block can use. */
export const vjFonts = () => invoke<VjFont[]>("vj_fonts");

/** The cameras and capture cards a Live input can show; empty when there are none. */
export const vjCaptureDevices = () => invoke<VjCaptureDevice[]>("vj_capture_devices");

/** What the open capture of a device says: connected, busy (held by another program), and whether it refused the format asked. */
export const vjCaptureStatus = (device: string) => invoke<VjCaptureStatus>("vj_capture_status", { device });

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
  /** For a text block that shows one line at a time: the line on screen now, from 0. */
  line: number | null;
}

export interface GridState {
  playing: PlayingClip[];
  /** The column launched last, while its clips play. */
  column: string | null;
  /** 0 is all side A, 1 all side B. */
  crossfader: number;
  /** The beats the grid's clock has counted, and whether they are held. */
  beats: number;
  beatsPaused: boolean;
}

/** What the operator does to the beat clock; none of it is saved. */
export type VjBeatOp = { op: "pause"; on: boolean } | { op: "resync" } | { op: "nudge"; direction: -1 | 0 | 1 };
export const vjBeat = (op: VjBeatOp) => invoke<GridState>("vj_beat", { op });

export const vjLaunch = (clip: string) => invoke<GridState>("vj_launch", { clip });
export const vjRelease = (clip: string) => invoke<GridState>("vj_release", { clip });
export const vjLaunchColumn = (column: string) => invoke<GridState>("vj_launch_column", { column });
/** Launches `column` on the layers of `group` only. */
export const vjLaunchGroupColumn = (group: string, column: string) => invoke<GridState>("vj_launch_group_column", { group, column });
/** Moves the playhead of the clip playing on `layer` to `position`, a fraction of the clip's length. */
export const vjScrub = (layer: string, position: number) => invoke<GridState>("vj_scrub", { layer, position });
/** Jumps to cue `index` (0 to 7) of a clip, launching the clip there if it is not playing. */
export const vjCue = (clip: string, index: number) => invoke<GridState>("vj_cue", { clip, index });
/** Stops a layer, or every layer with `null`. */
export const vjStop = (layer: string | null) => invoke<GridState>("vj_stop", { layer });
/** Puts the crossfader back where the backend starts it. */
export const vjCrossfaderReset = () => invoke<GridState>("vj_crossfader_reset");
export const vjCrossfader = (position: number) => invoke<GridState>("vj_crossfader", { position });
export const vjPlayState = () => invoke<GridState>("vj_play_state");
/** What the grid sends to the screens now, as a small picture (see `bindOutput`); empty outside a VJ project. */
export const vjOutputFrame = () => invoke<ArrayBuffer>("vj_output_frame");
export const vjScreenFrame = (screen: string) => invoke<ArrayBuffer>("vj_screen_frame", { screen });
/** What `clip` alone looks like in the composition, as a small picture (see `bindPreview`); it plays on its own, whatever is on air. */
export const vjPreviewFrame = (clip: string) => invoke<ArrayBuffer>("vj_preview_frame", { clip });
/** What one layer or group contributes to the composition, as a small picture (see `bindMonitor`). */
export const vjMonitorFrame = (kind: "layer" | "group", id: string) => invoke<ArrayBuffer>("vj_monitor_frame", { kind, id });

// ---- The dock layout ----
// Mirrors `vj_layout` in src-tauri/crates/flux-project/src/vj_layout.rs. The backend owns every rule of it.

/** The most monitors the backend lets be open (`MAX_MONITORS`); it refuses more. */
export const MAX_MONITORS = 8;
export type DockDir = "row" | "column";
export type DockNode =
  | { type: "split"; id: string; dir: DockDir; children: DockNode[]; sizes: number[] }
  | { type: "tabs"; id: string; panels: string[]; active: number }
  | { type: "deck" };
export type MonitorKind = { type: "composition" } | { type: "preview" } | { type: "layer"; layer: string | null } | { type: "group"; group: string | null } | { type: "screen"; screen: string };
export type DockZone = "center" | "left" | "right" | "top" | "bottom";
/** Where a panel is dropped: a tab group id or `"deck"`, and the zone of it. */
export interface DockTarget {
  node: string;
  zone: DockZone;
}
export interface DockLayout {
  version: number;
  root: DockNode;
  monitors: { id: string; kind: MonitorKind }[];
}
export type ClipSize = "small" | "medium" | "large";
export interface LayoutState {
  current: DockLayout;
  /** How big the grid's clips are drawn; kept per machine. */
  clipSize: ClipSize;
  saved: { id: string; name: string }[];
  factory: { id: "default" | "performance" | "monitors" }[];
  /** The least `[width, height]` in px of every split, tab group and `"deck"`. */
  mins: Record<string, [number, number]>;
  /** The gap a splitter takes, in px. */
  gap: number;
  /** For each monitor that shares a row with another one, the split of that row. */
  rowOf: Record<string, string>;
}
/** One command of the layout (`vj_layout_<op>`); each answers with the whole state. */
export const vjLayout = (op: string, args?: Record<string, unknown>) => invoke<LayoutState>(`vj_layout_${op}`, args);

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
  /** Needs the frames before it or the pixels around it: only the composition's stack takes it. */
  compositionOnly: boolean;
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
  /** The numbers of a generator, by name; a clip's modulation of one is the field `generator.<name>`. */
  generator: Record<string, VjModField>;
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
  /** The modulated knobs of the dashboard, by place. */
  knobs: Record<string, number>;
}

/** The colours a pad gets on the controller of `port` before the user changes them, and the name of its colour table (`apc`), if its values are colours. */
export const vjLightsPreset = (port?: string) => invoke<{ lights: VjLights; palette: string | null }>("vj_lights_preset", { port: port ?? null });

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

/** Waits for the next control touched, or OSC message, and makes it the source of `target`; `null` stops waiting. */
export const vjLearn = (target: VjTarget | null) => invoke<void>("vj_learn", { target });

/** What reading a `.flxmap` file would do: how much resolves, and the mappings it would overwrite. */
export interface MapImportPlan {
  taken: number;
  skipped: number;
  conflicts: { index: number; source: VjSource; existing: VjTarget; incoming: VjTarget }[];
}

/** What to do about the mappings an import overwrites: wipe all first, or keep the conflicts named (by `index`). */
export interface MapImportChoice {
  wipe: boolean;
  keep: number[];
}

export interface MapImported extends ProjectState {
  imported: number;
  skipped: number;
  kept: number;
}

export const vjMappingsExport = (project: Project, path: string) => invoke<void>("vj_mappings_export", { project, path });
export const vjMappingsImportPreview = (project: Project, path: string) =>
  invoke<MapImportPlan>("vj_mappings_import_preview", { project, path });
export const vjMappingsImport = (project: Project, path: string, choice: MapImportChoice) =>
  invoke<MapImported>("vj_mappings_import", { project, path, choice });

// Presets and templates: mirrors src-tauri/crates/flux-project/src/vj_presets.rs and src-tauri/src/vj_presets_commands.rs.

/** One effect of a preset: its settings as stored, `effect` naming the kind. */
export interface VjPresetEffect {
  settings: { effect: string } & Record<string, unknown>;
  bypass?: boolean;
}

/** A named set of effect settings kept for the user: one effect, or several (a stack). */
export interface VjPreset {
  id: string;
  name: string;
  effects: VjPresetEffect[];
}

/** What a preset is made from, or used on: one effect, or a whole stack. */
export type VjPresetSource = { effect: string } | { stack: { clip: string } | { layer: string } | { group: string } | "composition" };

/** What a template is listed as. */
export interface VjTemplate {
  id: string;
  name: string;
  layers: number;
  decks: number;
  columns: number;
}

export const vjPresetsList = () => invoke<VjPreset[]>("vj_presets_list");
export const vjPresetSave = (project: Project, source: VjPresetSource, name: string) =>
  invoke<VjPreset[]>("vj_preset_save", { project, source, name });
export const vjPresetRename = (id: string, name: string) => invoke<VjPreset[]>("vj_preset_rename", { id, name });
export const vjPresetDelete = (id: string) => invoke<VjPreset[]>("vj_preset_delete", { id });
export const vjPresetApply = (project: Project, id: string, to: VjPresetSource) =>
  invoke<ProjectState>("vj_preset_apply", { project, id, to });
export const vjTemplatesList = () => invoke<VjTemplate[]>("vj_templates_list");
export const vjTemplateSave = (project: Project, name: string) => invoke<VjTemplate[]>("vj_template_save", { project, name });
export const vjTemplateRename = (id: string, name: string) => invoke<VjTemplate[]>("vj_template_rename", { id, name });
export const vjTemplateDelete = (id: string) => invoke<VjTemplate[]>("vj_template_delete", { id });
export const vjTemplateNew = (id: string, name: string) => invoke<ProjectState>("vj_template_new", { id, name });
