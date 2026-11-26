// Setup navigation and the existing MIDI port display helpers.
import type { MtcOutputConfig } from "../project/types";

/**
 * The pages of the Setup dialog, gathered in the groups the page list folds into, in the order
 * they are listed. Audio holds every sound the show plays and where it goes; Outputs, the timecode and show
 * control the show sends out; Remote control, what drives FLUX
 * from outside.
 */
export const SETUP_GROUPS = [
  { id: "show", sections: ["project", "timeline"] },
  { id: "audio", sections: ["audio", "audioOutputs", "metronome", "operator", "analysis"] },
  { id: "outputs", sections: ["video", "ltc", "mtc", "osc"] },
  { id: "control", sections: ["timecodeIn", "controlMidi", "controlOsc", "http"] },
  { id: "app", sections: ["graphics", "general"] },
] as const;
export type SetupGroup = (typeof SETUP_GROUPS)[number]["id"];
export type SetupSection = (typeof SETUP_GROUPS)[number]["sections"][number];

/** Every page, in the order they are listed. */
export const SETUP_SECTIONS: readonly SetupSection[] = SETUP_GROUPS.flatMap((group) => group.sections);

/**
 * The pages a VJ show leaves out: it has no timeline, timecode, click or voice operator, and its video
 * output is set up in its own Advanced Output.
 */
const NOT_VJ: readonly SetupSection[] = ["timeline", "metronome", "operator", "ltc", "mtc", "osc", "video"];

/** The pages only a VJ show has: Flux is otherwise the master clock and reads no timecode, and only a VJ grid is modulated by sound. */
const ONLY_VJ: readonly SetupSection[] = ["timecodeIn", "analysis"];

/** Whether a page is listed for a show of this kind. */
export const isListed = (section: SetupSection, vj: boolean): boolean =>
  vj ? !NOT_VJ.includes(section) : !ONLY_VJ.includes(section);

/** HTTP is listed so the user can see it is planned, but has no page yet. */
export const isAvailable = (section: SetupSection): boolean => section !== "http";

/** The group a page is listed in. */
export const groupOf = (section: SetupSection): SetupGroup =>
  SETUP_GROUPS.find((group) => (group.sections as readonly SetupSection[]).includes(section))!.id;

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
