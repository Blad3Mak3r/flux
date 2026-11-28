import { fail } from "./fail";
import type { Translate } from "../i18n/messages";
import { exportCueSheet } from "../export/api";
import type { SheetScope } from "../export/api";
import { useDoc } from "./store";
import { say } from "./ui";

export async function exportCueSheetFile(scope: SheetScope, t: Translate) {
  try {
    if ((await exportCueSheet(useDoc.getState().project, scope, t)) !== null) say("status.cueSheetExported");
  } catch (err) {
    fail(err);
  }
}
