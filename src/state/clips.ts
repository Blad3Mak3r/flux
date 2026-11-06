// What the clip handles and the split key ask for. The backend does the work (it knows the length
// of the audio and what a trim or a fade may be) and answers with the project as it is now, which
// this only shows. Each answer is one step of undo.

import { clipEdit, errorMessage } from "../project/api";
import type { ClipOp } from "../project/api";
import { currentFrame } from "../transport/store";
import { activeTimeline } from "./doc";
import { dispatch, useDoc } from "./store";
import { say, sayError, useUi } from "./ui";

/** Asks for `op` at `frame` of the timeline (counted like a clip's `start`). Resolves when the answer is shown. */
export async function editClip(id: string, op: ClipOp, frame: number): Promise<void> {
  try {
    const { project, active } = await clipEdit(useDoc.getState().project, id, op, Math.max(0, frame));
    dispatch({ type: "apply", project, active });
  } catch (error) {
    sayError(errorMessage(error));
  }
}

/** Cuts the selected clip in two where the playhead is. */
export function splitAtPlayhead(): Promise<void> | void {
  const { selection } = useUi.getState();
  if (selection?.kind !== "clip") return say("status.splitNeedsClip");
  const { start } = activeTimeline(useDoc.getState()).timecode;
  return editClip(selection.id, "split", currentFrame() - start);
}
