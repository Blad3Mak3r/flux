// The DMX scenes and destinations. The backend does the work: it checks the channels, keeps them
// sorted and cleans up what pointed at a destination that went. It answers with the project as it
// is now, and this only shows it (and so it is one step of undo). Tracks and scenes are added,
// moved and removed with the cue operations in `./cues`.

import { dmxEdit, errorMessage } from "../project/api";
import type { DmxOp } from "../project/api";
import type { DmxProtocol } from "../project/types";
import { dispatch, useDoc } from "./store";
import { sayError } from "./ui";

/** Asks for the operation and shows the answer. Resolves to whether it was applied. */
async function run(op: DmxOp): Promise<boolean> {
  try {
    const answer = await dmxEdit(useDoc.getState().project, op);
    dispatch({ type: "apply", project: answer.project, active: answer.active });
    return true;
  } catch (error) {
    sayError(errorMessage(error));
    return false;
  }
}

export const setDmxScene = (id: string, name: string, fade: number, releaseOthers: boolean) =>
  run({ op: "setScene", id, name, fade, releaseOthers });
export const addDmxChannel = (id: string) => run({ op: "addChannel", id });
export const setDmxChannel = (
  id: string,
  from: { universe: number; channel: number },
  to: { universe: number; channel: number; value: number },
) => run({ op: "setChannel", id, fromUniverse: from.universe, fromChannel: from.channel, ...to });
export const removeDmxChannel = (id: string, universe: number, channel: number) =>
  run({ op: "removeChannel", id, universe, channel });
export const setDmxDestination = (trackId: string, destination: string | null) =>
  run({ op: "setDestination", trackId, destination });
export const addDmxDestination = (name: string, protocol: DmxProtocol, host: string) =>
  run({ op: "addDestination", name, protocol, host });
export const updateDmxDestination = (id: string, name: string, protocol: DmxProtocol, host: string) =>
  run({ op: "updateDestination", id, name, protocol, host });
export const removeDmxDestination = (id: string) => run({ op: "removeDestination", id });
