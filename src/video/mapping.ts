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
}

export type MappingOp =
  | { op: "add" }
  | { op: "remove"; id: string }
  | { op: "rename"; id: string; name: string }
  | { op: "setInput"; id: string; input: Rect }
  | { op: "setCorners"; id: string; corners: Slice["corners"] }
  | { op: "resetCorners"; id: string }
  | { op: "setHidden"; id: string; hidden: boolean }
  | { op: "clear" };

/** Sends one edit of the mapping to the backend; the answer enters the undo history. */
export async function mappingEdit(op: MappingOp): Promise<void> {
  try {
    const state = await invoke<ProjectState>("video_mapping_edit", { project: useDoc.getState().project, op });
    dispatch({ type: "apply", ...state });
  } catch (error) {
    sayError(errorMessage(error));
  }
}

/** Shows a test pattern on the output where there is no picture, to line the slices up with. */
export const showTestPattern = (on: boolean) => invoke("video_output_pattern", { on }).catch(() => undefined);
