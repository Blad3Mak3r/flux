import { useCallback, useEffect, useState } from "react";
import { errorMessage } from "../project/api";
import type { OutputConfig } from "../project/types";
import { findAudioDevice, findMidiPort, listAudioDevices, listAudioHosts, listMidiPorts } from "./api";
import type { AudioDevice, AudioHost, MidiPort } from "./api";
import { portKey, splitPortKey } from "../setup/outputs";

/** `found`: connected now. `missing`: the project names it but it is not connected. */
export type Presence = "unset" | "found" | "missing" | "unknown";

export interface Devices {
  /** The audio drivers of this system, the default first. */
  hosts: AudioHost[];
  /** The devices of the project's driver. */
  audio: AudioDevice[];
  midi: MidiPort[];
  midiError: string | null;
  /** The listing needs the desktop app; in a plain browser it never resolves. */
  available: boolean;
  /** The device that plays the programme and provides the clock. */
  audioPresence: Presence;
  ltcPresence: Presence;
  mtcPresence: Presence;
  refresh: () => void;
}

async function presenceOf(hint: string, find: (name: string) => Promise<unknown>): Promise<Presence> {
  // An empty name is the system's default output: there is nothing to look for.
  if (hint === "") return "found";
  try {
    return (await find(hint)) ? "found" : "missing";
  } catch {
    return "unknown";
  }
}

/** Several outputs read as one: a single missing device is what the user needs to hear about. */
export function worst(all: Presence[]): Presence {
  if (all.length === 0) return "unset";
  for (const state of ["missing", "unknown"] as const) if (all.includes(state)) return state;
  return "found";
}

/** Lists devices and checks whether the ones a project names are connected. */
export function useDevices(output: OutputConfig): Devices {
  const [hosts, setHosts] = useState<AudioHost[]>([]);
  const [audio, setAudio] = useState<AudioDevice[]>([]);
  const [midi, setMidi] = useState<MidiPort[]>([]);
  const [midiError, setMidiError] = useState<string | null>(null);
  const [available, setAvailable] = useState(true);
  const [audioPresence, setAudioPresence] = useState<Presence>("unset");
  const [ltcPresence, setLtcPresence] = useState<Presence>("unset");
  const [mtcPresence, setMtcPresence] = useState<Presence>("unset");
  const [tick, setTick] = useState(0);
  const refresh = useCallback(() => setTick((n) => n + 1), []);

  const driver = output.driver;
  const audioHint = output.audio?.deviceName ?? output.ltc[0]?.deviceName ?? null;
  // The effect depends on the names, not on the lists, which are new objects after every edit.
  const ltcHints = JSON.stringify(output.ltc.map((ltc) => ltc.deviceName));
  const midiHints = JSON.stringify(output.mtc.map(portKey));

  useEffect(() => {
    let cancelled = false;
    listAudioHosts()
      .then((list) => !cancelled && setHosts(list))
      .catch(() => !cancelled && setHosts([]));
    listAudioDevices(driver)
      .then((devices) => !cancelled && (setAudio(devices), setAvailable(true)))
      .catch(() => !cancelled && setAvailable(false));
    listMidiPorts()
      .then((ports) => !cancelled && (setMidi(ports), setMidiError(null)))
      .catch((err) => !cancelled && (setMidi([]), setMidiError(errorMessage(err))));
    return () => {
      cancelled = true;
    };
  }, [driver, tick]);

  useEffect(() => {
    let cancelled = false;
    const findAudio = (name: string) => findAudioDevice(name, driver);
    const resolveAll = (hints: (string | null)[], find: (name: string) => Promise<unknown>, set: (p: Presence) => void) => {
      if (hints.length === 0 || hints[0] === null) return set("unset");
      Promise.all(hints.map((hint) => presenceOf(hint as string, find))).then((all) => !cancelled && set(worst(all)));
    };
    resolveAll([audioHint], findAudio, setAudioPresence);
    resolveAll(JSON.parse(ltcHints) as string[], findAudio, setLtcPresence);
    resolveAll(JSON.parse(midiHints) as string[], (key) => findMidiPort(...splitPortKey(key)), setMtcPresence);
    return () => {
      cancelled = true;
    };
  }, [driver, audioHint, ltcHints, midiHints, tick]);

  return { hosts, audio, midi, midiError, available, audioPresence, ltcPresence, mtcPresence, refresh };
}
