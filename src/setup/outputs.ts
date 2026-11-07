// Setup navigation and the existing MIDI port display helpers.
import type { MtcOutputConfig } from "../project/types";

/** The pages of the Setup dialog, in the order they are listed. */
export const SETUP_SECTIONS = ["timeline", "project", "audio", "metronome", "ltc", "mtc", "osc", "control", "graphics", "general", "video"] as const;
export type SetupSection = (typeof SETUP_SECTIONS)[number];

/** Video is listed so the user can see it is planned, but has no page yet. */
export const isAvailable = (section: SetupSection): boolean => section !== "video";

/** The section `step` places from `from`, wrapping round and skipping the ones that are not available. */
export function stepSection(from: SetupSection, step: 1 | -1): SetupSection {
  let at = SETUP_SECTIONS.indexOf(from);
  for (let i = 0; i < SETUP_SECTIONS.length; i++) {
    at = (at + step + SETUP_SECTIONS.length) % SETUP_SECTIONS.length;
    if (isAvailable(SETUP_SECTIONS[at])) return SETUP_SECTIONS[at];
  }
  return from;
}

/** An MTC output's port as one string, `""` when none is chosen: the value of its select. */
export const portKey = ({ portName, portIndex = 0 }: MtcOutputConfig): string =>
  portName === "" ? "" : `${portIndex}|${portName}`;

/** The name and index a {@link portKey} stands for. */
export function splitPortKey(key: string): [string, number] {
  const at = key.indexOf("|");
  return at < 0 ? [key, 0] : [key.slice(at + 1), Number(key.slice(0, at))];
}

/** Each listed port with its place among the ports of the same name (0 for the first). */
export function portChoices(names: string[]): MtcOutputConfig[] {
  return names.map((portName, at) => {
    const portIndex = names.slice(0, at).filter((name) => name === portName).length;
    return portIndex > 0 ? { portName, portIndex } : { portName };
  });
}

/** How the picker names a port: ports that share a name are told apart by their number, "Port (2)". */
export function portLabel(choice: MtcOutputConfig, choices: MtcOutputConfig[]): string {
  const shared = choices.filter((other) => other.portName === choice.portName).length > 1;
  return shared ? `${choice.portName} (${(choice.portIndex ?? 0) + 1})` : choice.portName;
}
