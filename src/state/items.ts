// What the name fields, the fade curve selectors and the envelope points ask for. The backend does
// the work (it trims names, keeps the points in order and within their limits) and answers with the
// project as it is now, which this only shows. Each answer is one step of undo.

import { errorMessage, itemEdit, itemsEdit, itemsExtent } from "../project/api";
import type { ItemOp, ItemsOp } from "../project/api";
import { scrollLanesTo, useViewport } from "../timeline/layout";
import { frameX } from "../timeline/scale";
import { selectedIds } from "./selection";
import { dispatch, useDoc } from "./store";
import { sayError, setZoom, useUi } from "./ui";

/**
 * Asks for `op`. Resolves with the id of what it created (or `null`) when the answer is shown; a
 * refusal is said on the status line and resolves with `null`.
 */
export async function editItem(op: ItemOp): Promise<string | null> {
  try {
    const { project, active, id, changed } = await itemEdit(useDoc.getState().project, op);
    if (changed) dispatch({ type: "apply", project, active });
    return id;
  } catch (error) {
    sayError(errorMessage(error));
    return null;
  }
}

/** Asks for `op` on several items. Resolves when the answer is shown; a refusal is said on the status line. */
export async function editItems(op: ItemsOp): Promise<void> {
  try {
    const { project, active } = await itemsEdit(useDoc.getState().project, op);
    dispatch({ type: "apply", project, active });
  } catch (error) {
    sayError(errorMessage(error));
  }
}

/** Moves the items by `delta` frames, with the ripple mode on: one step of undo. */
export const moveItems = (ids: string[], delta: number) =>
  editItems({ op: "move", ids, delta, ripple: useUi.getState().ripple });

/** Removes the items, with the ripple mode on: one step of undo. */
export const removeItems = (ids: string[]) =>
  editItems({ op: "remove", ids, ripple: useUi.getState().ripple });

/** Zooms so that the selected items fill the view, or the whole timeline when none are selected. */
export async function zoomToFit(): Promise<void> {
  const ids = selectedIds();
  try {
    const extent = await itemsExtent(useDoc.getState().project, ids);
    const width = useViewport.getState().width;
    if (!extent || width <= 0) return;
    // A margin each side, and at least a second, so a single point does not zoom to the limit.
    const frames = Math.max(extent.to - extent.from, 25);
    const margin = width * 0.05;
    setZoom(((width - 2 * margin) / frames));
    scrollLanesTo(frameX(extent.from, useUi.getState().zoom) - margin);
  } catch (error) {
    sayError(errorMessage(error));
  }
}
