// Pure edits of the output lists the Setup dialog works on. Like `state/edit.ts`, every function
// returns a new `OutputConfig` and leaves its input alone, so the undo history can keep the old one.

import type { LtcOutputConfig, MtcOutputConfig, OutputConfig } from "../project/types";

/** LTC goes on the highest channel up to 8, which interfaces conventionally keep for sync. */
export const DEFAULT_LTC_CHANNEL_CAP = 8;

/** The pages of the Setup dialog, in the order they are listed. */
export const SETUP_SECTIONS = ["timeline", "project", "audio", "metronome", "ltc", "mtc", "osc", "general", "video"] as const;
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

/** The highest channel up to the cap that is not in `taken`, or the cap when every one is. */
export function freeChannel(taken: number[], channelCount: number): number {
  const top = Math.max(1, Math.min(DEFAULT_LTC_CHANNEL_CAP, channelCount));
  for (let channel = top; channel >= 1; channel--) if (!taken.includes(channel)) return channel;
  return top;
}

/** A new LTC output on `deviceName`, on a channel neither the programme nor another LTC there uses. */
export function addLtc(output: OutputConfig, deviceName: string, channelCount: number): OutputConfig {
  const onDevice = output.ltc.filter((ltc) => ltc.deviceName === deviceName).map((ltc) => ltc.channel);
  const programme = output.audio?.deviceName === deviceName ? output.audio.channels : [];
  const channel = freeChannel([...onDevice, ...programme], channelCount);
  return { ...output, ltc: [...output.ltc, { deviceName, channel }] };
}

export function updateLtc(output: OutputConfig, index: number, change: Partial<LtcOutputConfig>): OutputConfig {
  return { ...output, ltc: output.ltc.map((ltc, i) => (i === index ? { ...ltc, ...change } : ltc)) };
}

export function removeLtc(output: OutputConfig, index: number): OutputConfig {
  return { ...output, ltc: output.ltc.filter((_, i) => i !== index) };
}

/** The LTC level of an output that sets none, as the backend reads it. */
export const DEFAULT_LTC_LEVEL = 0.5;

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

/** A new MTC output on the first listed port that no other MTC output uses yet, or none chosen. */
export function addMtc(output: OutputConfig, ports: string[]): OutputConfig {
  const used = output.mtc.map(portKey);
  const mtc = portChoices(ports).find((choice) => !used.includes(portKey(choice))) ?? { portName: "" };
  return { ...output, mtc: [...output.mtc, mtc] };
}

/** Points the MTC output at the port a {@link portKey} stands for. */
export function setMtcPort(output: OutputConfig, index: number, key: string): OutputConfig {
  const [portName, portIndex] = splitPortKey(key);
  const { portIndex: _previous, ...rest } = output.mtc[index] ?? { portName };
  const mtc = portIndex > 0 ? { ...rest, portName, portIndex } : { ...rest, portName };
  return { ...output, mtc: output.mtc.map((other, i) => (i === index ? mtc : other)) };
}

export function updateMtc(output: OutputConfig, index: number, change: Partial<MtcOutputConfig>): OutputConfig {
  return { ...output, mtc: output.mtc.map((mtc, i) => (i === index ? { ...mtc, ...change } : mtc)) };
}

export function removeMtc(output: OutputConfig, index: number): OutputConfig {
  return { ...output, mtc: output.mtc.filter((_, i) => i !== index) };
}

/**
 * Moves every audio output to another driver. Device names belong to one driver, so each goes back
 * to that driver's default device, keeping its channels. The system's default driver is stored as
 * no driver, so the project does not pin what this machine would pick anyway.
 */
export function setDriver(output: OutputConfig, driver: string | undefined): OutputConfig {
  const { driver: _driver, ...rest } = output;
  const system = <T extends { deviceName: string }>(device: T): T => ({ ...device, deviceName: "" });
  return {
    ...rest,
    ...(driver ? { driver } : {}),
    audio: output.audio && system(output.audio),
    ...(output.preview ? { preview: system(output.preview) } : {}),
    ...(output.metronome ? { metronome: system(output.metronome) } : {}),
    ltc: output.ltc.map(system),
  };
}

/** Moves the programme to another device. LTC outputs that were on the old one move with it. */
export function setProgrammeDevice(output: OutputConfig, deviceName: string, channelCount: number): OutputConfig {
  const before = output.audio?.deviceName ?? output.ltc[0]?.deviceName ?? "";
  return {
    ...output,
    audio: { deviceName, channels: channelCount >= 2 ? [1, 2] : [1] },
    ltc: output.ltc.map((ltc) =>
      ltc.deviceName === before
        ? { ...ltc, deviceName, channel: Math.min(ltc.channel, Math.max(1, channelCount)) }
        : ltc,
    ),
  };
}

/** Auditions Edit on `deviceName`, on its first pair. Unlike the programme, nothing else follows it. */
export function setPreviewDevice(output: OutputConfig, deviceName: string, channelCount: number): OutputConfig {
  return { ...output, preview: { deviceName, channels: channelCount >= 2 ? [1, 2] : [1] } };
}

/** Edit plays on the programme again. */
export function clearPreview(output: OutputConfig): OutputConfig {
  const { preview: _preview, ...rest } = output;
  return rest;
}

/** Plays the metronome on `deviceName`, on its first pair, keeping the level it had. */
export function setMetronomeDevice(output: OutputConfig, deviceName: string, channelCount: number): OutputConfig {
  const channels = channelCount >= 2 ? [1, 2] : [1];
  return { ...output, metronome: { ...output.metronome, deviceName, channels } };
}

/** The metronome goes to `channels` of its device. A project with no metronome output stays without one. */
export function setMetronomeChannels(output: OutputConfig, channels: number[]): OutputConfig {
  return output.metronome ? { ...output, metronome: { ...output.metronome, channels } } : output;
}

/** The level of the click in dB. 0 dB is stored as no value, like a track's fader. */
export function setMetronomeGain(output: OutputConfig, gainDb: number): OutputConfig {
  if (!output.metronome) return output;
  const { gainDb: _previous, ...rest } = output.metronome;
  return { ...output, metronome: gainDb === 0 ? rest : { ...rest, gainDb } };
}

/** No click plays. */
export function clearMetronome(output: OutputConfig): OutputConfig {
  const { metronome: _metronome, ...rest } = output;
  return rest;
}

/** Whether a rendered device is among the connected ones (an empty name is the system default). */
export const isConnected = (name: string, connected: string[]): boolean => name === "" || connected.includes(name);
