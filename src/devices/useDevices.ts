import { useCallback, useEffect, useState } from "react";
import { errorMessage } from "../project/api";
import type { OutputConfig } from "../project/types";
import { findAudioDevice, findMidiPort, listAudioDevices, listMidiPorts } from "./api";
import type { AudioDevice, MidiPort } from "./api";

/** `found`: connected now. `missing`: the project names it but it is not connected. */
export type Presence = "unset" | "found" | "missing" | "unknown";

export interface Devices {
  audio: AudioDevice[];
  midi: MidiPort[];
  midiError: string | null;
  /** The listing needs the desktop app; in a plain browser it never resolves. */
  available: boolean;
  audioPresence: Presence;
  mtcPresence: Presence;
  refresh: () => void;
}

/** Lists devices and checks whether the ones a project names are connected. */
export function useDevices(output: OutputConfig): Devices {
  const [audio, setAudio] = useState<AudioDevice[]>([]);
  const [midi, setMidi] = useState<MidiPort[]>([]);
  const [midiError, setMidiError] = useState<string | null>(null);
  const [available, setAvailable] = useState(true);
  const [audioPresence, setAudioPresence] = useState<Presence>("unset");
  const [mtcPresence, setMtcPresence] = useState<Presence>("unset");
  const [tick, setTick] = useState(0);
  const refresh = useCallback(() => setTick((n) => n + 1), []);

  const audioHint = output.ltc?.deviceName ?? output.audio?.deviceName ?? null;
  const midiHint = output.mtc?.portName ?? null;

  useEffect(() => {
    let cancelled = false;
    listAudioDevices()
      .then((devices) => !cancelled && (setAudio(devices), setAvailable(true)))
      .catch(() => !cancelled && setAvailable(false));
    listMidiPorts()
      .then((ports) => !cancelled && (setMidi(ports), setMidiError(null)))
      .catch((err) => !cancelled && (setMidi([]), setMidiError(errorMessage(err))));
    return () => {
      cancelled = true;
    };
  }, [tick]);

  useEffect(() => {
    let cancelled = false;
    const resolve = (hint: string | null, find: (name: string) => Promise<unknown>, set: (p: Presence) => void) => {
      if (hint === null) return set("unset");
      find(hint)
        .then((found) => !cancelled && set(found ? "found" : "missing"))
        .catch(() => !cancelled && set("unknown"));
    };
    resolve(audioHint, findAudioDevice, setAudioPresence);
    resolve(midiHint, findMidiPort, setMtcPresence);
    return () => {
      cancelled = true;
    };
  }, [audioHint, midiHint, tick]);

  return { audio, midi, midiError, available, audioPresence, mtcPresence, refresh };
}
