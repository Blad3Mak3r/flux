// What the name fields, the fade curve selectors and the envelope points ask for. The backend does
// the work (it trims names, keeps the points in order and within their limits) and answers with the
// project as it is now, which this only shows. Each answer is one step of undo.

import { errorMessage, itemEdit } from "../project/api";
import type { ItemOp } from "../project/api";
import { dispatch, useDoc } from "./store";
import { sayError } from "./ui";

/** Asks for `op`. Resolves when the answer is shown; a refusal is said on the status line. */
export async function editItem(op: ItemOp): Promise<void> {
  try {
    const { project, active } = await itemEdit(useDoc.getState().project, op);
    dispatch({ type: "apply", project, active });
  } catch (error) {
    sayError(errorMessage(error));
  }
}
