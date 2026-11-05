// Pure edits of the output lists the Setup dialog works on. Like `state/edit.ts`, every function
// returns a new `OutputConfig` and leaves its input alone, so the undo history can keep the old one.

import type { LtcOutputConfig, MtcOutputConfig, OutputConfig } from "../project/types";

/** LTC goes on the highest channel up to 8, which interfaces conventionally keep for sync. */
export const DEFAULT_LTC_CHANNEL_CAP = 8;

/** The pages of the Setup dialog, in the order they are listed. */
export const SETUP_SECTIONS = ["timeline", "project", "audio", "ltc", "mtc", "general", "video"] as const;
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

/** A new MTC output on the first listed port that no other MTC output uses yet, or none chosen. */
export function addMtc(output: OutputConfig, ports: string[]): OutputConfig {
  const used = output.mtc.map((mtc) => mtc.portName);
  const portName = ports.find((port) => !used.includes(port)) ?? "";
  return { ...output, mtc: [...output.mtc, { portName }] };
}

export function updateMtc(output: OutputConfig, index: number, change: Partial<MtcOutputConfig>): OutputConfig {
  return { ...output, mtc: output.mtc.map((mtc, i) => (i === index ? { ...mtc, ...change } : mtc)) };
}

export function removeMtc(output: OutputConfig, index: number): OutputConfig {
  return { ...output, mtc: output.mtc.filter((_, i) => i !== index) };
}

/** Moves the programme to another device. LTC outputs that were on the old one move with it. */
export function setProgrammeDevice(output: OutputConfig, deviceName: string, channelCount: number): OutputConfig {
  const before = output.audio?.deviceName ?? output.ltc[0]?.deviceName ?? "";
  return {
    ...output,
    audio: { deviceName, channels: channelCount >= 2 ? [1, 2] : [1] },
    ltc: output.ltc.map((ltc) =>
      ltc.deviceName === before ? { deviceName, channel: Math.min(ltc.channel, Math.max(1, channelCount)) } : ltc,
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

/** Whether a rendered device is among the connected ones (an empty name is the system default). */
export const isConnected = (name: string, connected: string[]): boolean => name === "" || connected.includes(name);
