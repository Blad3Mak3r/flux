import type { Translate } from "../i18n/messages";
import { exportCueSheet } from "../export/api";
import type { SheetScope } from "../export/api";
import { errorMessage } from "../project/api";
import { useDoc } from "./store";
import { say, sayError } from "./ui";

export async function exportCueSheetFile(scope: SheetScope, t: Translate) {
  try {
    if ((await exportCueSheet(useDoc.getState().project, scope, t)) !== null) say("status.cueSheetExported");
  } catch (err) {
    sayError(errorMessage(err));
  }
}
