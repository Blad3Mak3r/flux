import type { Translate } from "../i18n/messages";
import type { Cue } from "./api";

/** What to call a cue: its own name, or its kind when a note has none. */
export function cueName(cue: Cue, t: Translate): string {
  return cue.name.trim() === "" ? t(`live.kind.${cue.kind}`) : cue.name;
}
