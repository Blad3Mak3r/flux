// The MIDI and OSC tracks and the OSC destinations. The backend does the work: it builds the event,
// checks it, keeps the events in order and cleans up what pointed at a destination that went. It
// answers with the project as it is now, and this only shows it (and so it is one step of undo).

import { cueEdit, errorMessage } from "../project/api";
import type { CueOp, CueTrackKind } from "../project/api";
import type { MidiMessage, MidiPortRef, OscArg } from "../project/types";
import { activeTimeline } from "./doc";
import { dispatch, useDoc } from "./store";
import { select, sayError } from "./ui";

/** Asks for the operation and shows the answer. Resolves to the id of what it created. */
async function run(op: CueOp): Promise<string | null> {
  try {
    const answer = await cueEdit(useDoc.getState().project, op);
    dispatch({ type: "apply", project: answer.project, active: answer.active });
    return answer.id;
  } catch (error) {
    sayError(errorMessage(error));
    return null;
  }
}

/** Adds an empty MIDI or OSC track and selects nothing: the user picks its output next. */
export const addCueTrack = (kind: CueTrackKind, name: string) => run({ op: "addTrack", kind, name });

/** Adds an event at `frame` (absolute, as the playhead counts) and selects it to be edited. */
export async function addCueEvent(kind: CueTrackKind, frame: number, trackName: string, trackId?: string): Promise<void> {
  const { start } = activeTimeline(useDoc.getState()).timecode;
  const id = await run({ op: "addEvent", kind, trackId, trackName, at: Math.max(0, frame - start) });
  if (id) select({ kind: "event", id });
}

export const moveCueEvent = (id: string, at: number) => run({ op: "moveEvent", id, at: Math.max(0, at) });
export const removeCueEvent = (id: string) => run({ op: "removeEvent", id });
export const setMidiMessage = (id: string, message: MidiMessage) => run({ op: "setMidi", id, message });
export const setOscMessage = (id: string, address: string, args: OscArg[]) =>
  run({ op: "setOsc", id, address, args });
export const setMidiPort = (trackId: string, port: MidiPortRef | null) => run({ op: "setMidiPort", trackId, port });
export const setOscDestination = (trackId: string, destination: string | null) =>
  run({ op: "setOscDestination", trackId, destination });
export const addDestination = (name: string, host: string, port: number) =>
  run({ op: "addDestination", name, host, port });
export const updateDestination = (id: string, name: string, host: string, port: number) =>
  run({ op: "updateDestination", id, name, host, port });
export const removeDestination = (id: string) => run({ op: "removeDestination", id });
