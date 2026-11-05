// What the backend says about the show: the cue list and where the transport stands among the
// cues. The state changes about 30 times a second while playing, so components read one slice of
// it through a selector.

import { create } from "zustand";
import type { Project } from "../project/types";
import type { Cue, LiveState } from "./api";

export interface LiveStore {
  cues: Cue[];
  state: LiveState | null;
  /** The backend has answered for the project the cues belong to; until then an empty list means "not known yet". */
  ready: boolean;
  /** What the cues were derived from, to tell whether they still describe the open project. */
  source: Pick<Project, "tracks" | "assets" | "timecode"> | null;
}

export const useLive = create<LiveStore>()(() => ({ cues: [], state: null, ready: false, source: null }));
