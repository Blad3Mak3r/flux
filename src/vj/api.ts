import { invoke } from "@tauri-apps/api/core";
import type {
  Project,
  ProjectState,
  VideoFit,
  VjBlend,
  VjPlayback,
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
  | { op: "layerBypass" | "layerSolo"; layer: string; on: boolean }
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
  | { op: "moveClip"; clip: string; layer: string; column: string }
  | { op: "removeClip"; clip: string }
  | { op: "renameClip"; clip: string; name: string }
  | { op: "clipPlayback"; clip: string; playback: VjPlayback }
  | { op: "clipTrigger"; clip: string; trigger: VjTrigger }
  | { op: "clipFit"; clip: string; fit: VideoFit }
  | { op: "clipScale"; clip: string; value: number }
  | { op: "clipPosition"; clip: string; x: number; y: number };

export const vjEdit = (project: Project, op: VjOp) => invoke<ProjectState>("vj_edit", { project, op });

/** Adds the video at `path` to the cell of `layer` in `column`, or to its first empty cell in the deck shown. */
export const vjImport = (project: Project, path: string, layer: string, column: string | null) =>
  invoke<ProjectState>("project_import_vj_video", { project, path, layer, column });

/** A clip playing on a layer, as the backend answers. */
export interface PlayingClip {
  layer: string;
  clip: string;
  /** Since it was launched. */
  seconds: number;
  /** Where it is in its video, 0 to 1; `null` until its length is known. */
  progress: number | null;
  /** Seconds until the end of its video. */
  remaining: number | null;
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
export const vjStop = (layer: string | null) => invoke<GridState>("vj_stop", { layer });
export const vjCrossfader = (position: number) => invoke<GridState>("vj_crossfader", { position });
export const vjPlayState = () => invoke<GridState>("vj_play_state");
/** What the grid sends to the screens now, as a small picture (see `bindOutput`); empty outside a VJ project. */
export const vjOutputFrame = () => invoke<ArrayBuffer>("vj_output_frame");
