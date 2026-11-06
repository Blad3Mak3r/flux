// What the output rows of Setup ask for. The backend reads and checks what was typed and answers
// with the project as it is now, which this only shows.

import { errorMessage, outputSetOffset } from "../project/api";
import type { OutputKind } from "../project/api";
import { dispatch, useDoc } from "./store";
import { sayError } from "./ui";

/** Sets the latency offset of the LTC or MTC output at `index` to the ms in `text`. */
export async function setOutputOffset(kind: OutputKind, index: number, text: string): Promise<void> {
  try {
    const { project, active } = await outputSetOffset(useDoc.getState().project, kind, index, text);
    dispatch({ type: "apply", project, active });
  } catch (error) {
    sayError(errorMessage(error));
  }
}
