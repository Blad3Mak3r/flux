import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Mirrors src-tauri/src/control.rs and src-tauri/crates/flux-engine/src/control.rs. The backend
// listens, decodes, maps and runs the actions; the window shows the choices and forwards them.

export type ControlAction =
  | { type: "go" }
  | { type: "devamp" }
  | { type: "cancelDevamp" }
  | { type: "play" }
  | { type: "pause" }
  | { type: "toggle" }
  | { type: "stop" }
  | { type: "nextCue" }
  | { type: "prevCue" }
  | { type: "gotoCue"; value: number }
  | { type: "stepFrames"; value: number }
  | { type: "stepSeconds"; value: number }
  | { type: "nextTimeline" }
  | { type: "prevTimeline" }
  | { type: "selectTimeline"; value: number }
  | { type: "viewEdit" }
  | { type: "viewLive" };

export type ActionType = ControlAction["type"];

export interface MidiTrigger {
  kind: "note" | "cc" | "program";
  /** From 1 to 16. */
  channel: number;
  number: number;
}

/** What a pad is sent to light it and to put it out: a velocity for a note, a value for a control change. */
export interface Feedback {
  on: number;
  off: number;
}

export interface Binding {
  trigger: MidiTrigger;
  action: ControlAction;
  /** Absent: the backend uses the preset of the controller (`ControlView.feedbackDefault`). */
  feedback?: Feedback;
}

export interface ControlPrefs {
  midi: { enabled: boolean; portName: string | null; portIndex: number; bindings: Binding[]; feedback?: boolean };
  osc: { enabled: boolean; port: number; bindAddress?: string; tcpEnabled?: boolean };
}

/** Whether an input is listening, and if not, why. `detail` is the port, or the reason of an error. */
export type LinkState =
  | { state: "off" }
  | { state: "missing" }
  | { state: "listening"; detail: string }
  | { state: "error"; detail: string };

export interface ControlView {
  prefs: ControlPrefs;
  midi: LinkState;
  osc: LinkState;
  /** The MIDI input ports there are now. */
  inputs: { name: string }[];
  /** Why MIDI cannot be listed, on a system without it. */
  midiError: string | null;
  /** The values a binding without its own lights its pad with, for the chosen controller. */
  feedbackDefault: Feedback;
}

/** What only the window can do for the remote control. */
export type WindowAction = "play" | "viewEdit" | "viewLive";

export const controlGet = () => invoke<ControlView>("control_get");
/** Keeps the preferences and opens the inputs they ask for; sending them again retries a missing port. */
export const controlSet = (prefs: ControlPrefs) => invoke<ControlView>("control_set", { prefs });
/** While on, the next trigger is told to the window with `control-learned` instead of being run. */
export const controlLearn = (on: boolean) => invoke<void>("control_learn", { on });

const hear = <T>(event: string, handler: (payload: T) => void): (() => void) => {
  // Outside the desktop app there is no backend to hear from.
  const listening = listen<T>(event, ({ payload }) => handler(payload)).catch(() => undefined);
  return () => void listening.then((unlisten) => unlisten?.());
};

export const onLearned = (handler: (trigger: MidiTrigger) => void) => hear("control-learned", handler);
export const onWindowAction = (handler: (action: WindowAction) => void) => hear("control-action", handler);
