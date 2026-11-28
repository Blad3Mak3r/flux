// What the Output menu switches on the screens: disable, test card, full screen on a display. The backend owns them; this
// keeps the last state it answered with and asks again now and then, since a window can be closed from outside.

import { invoke } from "@tauri-apps/api/core";
import { create } from "zustand";
import { watch } from "../watch";
import { errorMessage } from "../project/api";
import { sayError } from "../state/ui";

/** Mirrors `OutputState` in src-tauri/src/video_output.rs. */
export interface OutputState {
  /** Every screen is black. */
  disabled: boolean;
  /** Every screen shows the test card. */
  testCard: boolean;
  /** How many displays there are to cover. */
  displays: number;
  /** The display (from 1) the main screen covers; none while it is a window. */
  fullscreen: number | null;
}

/** What the saved snapshot is, as `video_output_snapshot` answers. */
export interface Snapshot {
  path: string;
  width: number;
  height: number;
}

export const useOutput = create<OutputState>(() => ({ disabled: false, testCard: false, displays: 1, fullscreen: null }));

const fail = (err: unknown) => sayError(errorMessage(err));
const accept = (state: OutputState) => useOutput.setState(state);

/** Keeps the state up to date while the VJ screen is open: the backend sends it when it changes. */
export const bindOutputState = () => watch<OutputState>("video_output_watch", accept);

export const toggleDisable = () => void invoke<OutputState>("video_output_toggle", { control: "disable" }).then(accept, fail);
export const toggleTestCard = () => void invoke<OutputState>("video_output_toggle", { control: "testCard" }).then(accept, fail);

/** Covers display `display` (from 1) with the output, or goes back to a window when it is the one covered or `null`. */
export function fullscreenOn(display: number | null): void {
  const next = display === useOutput.getState().fullscreen ? null : display;
  void invoke<OutputState>("video_output_fullscreen", { display: next }).then(accept, fail);
}

export const saveSnapshot = () => invoke<Snapshot>("video_output_snapshot");
