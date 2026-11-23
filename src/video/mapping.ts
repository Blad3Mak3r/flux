// The slices of the video output, edited in the backend (src-tauri/crates/flux-project/src/mapping.rs).

import { invoke } from "@tauri-apps/api/core";
import { errorMessage } from "../project/api";
import type { ProjectState } from "../project/types";
import { dispatch, useDoc } from "../state/store";
import { sayError } from "../state/ui";

export interface Point {
  x: number;
  y: number;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** A rectangle of the picture (`input`) placed on the screen by its four corners, all as 0 to 1 from the top left. */
export interface Slice {
  id: string;
  name: string;
  input: Rect;
  /** Top left, top right, bottom right, bottom left. */
  corners: [Point, Point, Point, Point];
  hidden?: boolean;
  /** How far in from each edge it fades to black, as fractions of it, and the fade's curve. */
  softEdge?: SoftEdge;
}

export interface SoftEdge {
  left: number;
  right: number;
  top: number;
  bottom: number;
  gamma: number;
}

/** A shape on a screen that hides the picture inside it, or outside it when inverted. */
export interface Mask {
  id: string;
  name: string;
  points: Point[];
  invert?: boolean;
  hidden?: boolean;
}

export type MaskOp =
  | { op: "add" }
  | { op: "remove"; id: string }
  | { op: "rename"; id: string; name: string }
  | { op: "setPoints"; id: string; points: Point[] }
  | { op: "addPoint"; id: string; after: number }
  | { op: "removePoint"; id: string; index: number }
  | { op: "setInvert"; id: string; invert: boolean }
  | { op: "setHidden"; id: string; hidden: boolean };

/** A picture's or a screen's size in pixels; typed values are pixels of it. */
export interface Size {
  width: number;
  height: number;
}

/** A window of the output on a display of its own, with its slices. The main one always exists. */
export interface VideoScreen {
  id: string;
  name: string;
  slices?: Slice[];
  masks?: Mask[];
}

export const MAIN_SCREEN = "main";

export type ScreenOp =
  | { op: "add" }
  | { op: "remove"; id: string }
  | { op: "rename"; id: string; name: string };

export type MappingOp =
  | { op: "add" }
  | { op: "remove"; id: string }
  | { op: "rename"; id: string; name: string }
  | { op: "setInput"; id: string; input: Rect }
  | { op: "setCorners"; id: string; corners: Slice["corners"] }
  | {
      op: "setInputValue";
      id: string;
      field: "x" | "y" | "width" | "height";
      text: string;
      size: Size;
    }
  | {
      op: "setCornerValue";
      id: string;
      corner: number;
      axis: "x" | "y";
      text: string;
      size: Size;
    }
  | {
      op: "setSoftEdge";
      id: string;
      field: "left" | "right" | "top" | "bottom" | "gamma";
      text: string;
    }
  | { op: "resetCorners"; id: string }
  | { op: "setHidden"; id: string; hidden: boolean }
  | { op: "clear" };

/** Sends one edit of a screen's slices to the backend; the answer enters the undo history. */
export async function mappingEdit(
  screen: string,
  op: MappingOp,
): Promise<void> {
  try {
    const state = await invoke<ProjectState>("video_mapping_edit", {
      project: useDoc.getState().project,
      screen,
      op,
    });
    dispatch({ type: "apply", ...state });
  } catch (error) {
    sayError(errorMessage(error));
  }
}

/** Sends one edit of a screen's masks to the backend; the answer enters the undo history. */
export async function maskEdit(screen: string, op: MaskOp): Promise<void> {
  try {
    const state = await invoke<ProjectState>("video_mask_edit", {
      project: useDoc.getState().project,
      screen,
      op,
    });
    dispatch({ type: "apply", ...state });
  } catch (error) {
    sayError(errorMessage(error));
  }
}

/** Where the size of the composition comes from, when the project does not set it. */
const DEFAULT_COMPOSITION = { width: 1920, height: 1080 };

/**
 * The size of the composition the clips are sized into and the slices cut, as the output uses it:
 * the one the project sets, else its first video's, else 1920 × 1080.
 */
export function useComposition(): { width: number; height: number; set: boolean } {
  const set = useDoc((doc) => doc.project.output.composition);
  const video = useDoc((doc) => doc.video[doc.project.videos?.[0]?.id ?? ""]);
  if (set) return { ...set, set: true };
  return video ? { width: video.width, height: video.height, set: false } : { ...DEFAULT_COMPOSITION, set: false };
}

/** Sets the size of the composition; both sides empty go back to the first video's size. */
export async function compositionSet(width: string, height: string): Promise<void> {
  try {
    const state = await invoke<ProjectState>("video_composition_set", {
      project: useDoc.getState().project,
      width,
      height,
    });
    dispatch({ type: "apply", ...state });
  } catch (error) {
    sayError(errorMessage(error));
  }
}

/** Adds, removes or renames a screen; its window opens or closes on its own. */
export async function screenEdit(op: ScreenOp): Promise<void> {
  try {
    const state = await invoke<ProjectState>("video_screen_edit", {
      project: useDoc.getState().project,
      op,
    });
    dispatch({ type: "apply", ...state });
  } catch (error) {
    sayError(errorMessage(error));
  }
}

/** Shows a test pattern on the output where there is no picture, to line the slices up with. */
export const showTestPattern = (on: boolean) =>
  invoke("video_output_pattern", { on }).catch(() => undefined);
