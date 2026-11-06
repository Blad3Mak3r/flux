import type { Translate } from "../i18n/messages";
import type { Cue } from "./api";

/** What to call a cue: its own name, what a MIDI or OSC cue sends with its notation, or its kind when a note has none. */
export function cueName(cue: Cue, t: Translate): string {
  if (cue.action) return cue.detail ? `${t(`live.action.${cue.action}`)} · ${cue.detail}` : t(`live.action.${cue.action}`);
  return cue.name.trim() === "" ? t(`live.kind.${cue.kind}`) : cue.name;
}
