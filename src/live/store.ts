// What the backend says about the show: the cue list and where the transport stands among the
// cues. The state changes about 30 times a second while playing, so components read one slice of
// it through a selector.

import { create } from "zustand";
import type { Cue, LiveState } from "./api";

export interface LiveStore {
  cues: Cue[];
  state: LiveState | null;
}

export const useLive = create<LiveStore>()(() => ({ cues: [], state: null }));
