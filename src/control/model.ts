// What the Remote control page needs to show and to hand back to the backend. The backend decodes,
// maps, validates and runs the actions (it drops a binding that cannot work); nothing here decides
// what a message means.

import type { ActionType, Binding, ControlAction, ControlPrefs, Feedback, MidiTrigger } from "./api";

/** Every action, in the order the picker lists them. */
export const ACTION_TYPES: ActionType[] = [
  "go", "devamp", "cancelDevamp",
  "play",
  "pause",
  "toggle",
  "stop",
  "nextCue",
  "prevCue",
  "gotoCue",
  "stepFrames",
  "stepSeconds",
  "nextTimeline",
  "prevTimeline",
  "selectTimeline",
  "viewEdit",
  "viewLive",
];

/** The actions that carry a number, and the lowest one they accept (`null`: any but zero, negative goes back). */
const NUMBERED: Partial<Record<ActionType, number | null>> = {
  gotoCue: 1,
  selectTimeline: 1,
  stepFrames: null,
  stepSeconds: null,
};

export const isNumbered = (type: ActionType): boolean => type in NUMBERED;

/** The action of `type`, with the number a new one starts at. */
export function defaultAction(type: ActionType): ControlAction {
  return (isNumbered(type) ? { type, value: 1 } : { type }) as ControlAction;
}

/** The action with another number; one without a number is returned as it is. */
export const withValue = (action: ControlAction, value: number): ControlAction =>
  "value" in action ? { ...action, value } : action;

/** Whether `value` is a number the action accepts, for the field to refuse it early. */
export function acceptsValue(type: ActionType, value: number): boolean {
  if (!Number.isInteger(value)) return false;
  const lowest = NUMBERED[type];
  return lowest === null ? value !== 0 : value >= (lowest ?? 0);
}

const NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/** A note number as musicians write it; 60 is C4, as the MIDI cue tracks show it. */
export const noteName = (note: number): string => `${NOTE_NAMES[note % 12]}${Math.floor(note / 12) - 1}`;

/** A trigger as a controller's manual would write it: not a sentence, so not translated. */
export function triggerLabel({ kind, channel, number }: MidiTrigger): string {
  const what = kind === "note" ? `Note ${noteName(number)}` : kind === "cc" ? `CC ${number}` : `Program ${number}`;
  return `${what} · ch ${channel}`;
}

/**
 * The preferences with a new binding. It goes first: the backend keeps one action per trigger and
 * the first one wins, so learning a pad that already had an action takes it over.
 */
export const addBinding = (prefs: ControlPrefs, binding: Binding): ControlPrefs => ({
  ...prefs,
  midi: { ...prefs.midi, bindings: [binding, ...prefs.midi.bindings] },
});

export const setBindingAction = (prefs: ControlPrefs, index: number, action: ControlAction): ControlPrefs => ({
  ...prefs,
  midi: { ...prefs.midi, bindings: prefs.midi.bindings.map((binding, i) => (i === index ? { ...binding, action } : binding)) },
});

/** Whether `value` fits a MIDI data byte, for the field of a lit or unlit value to refuse it early. */
export const acceptsFeedback = (value: number): boolean => Number.isInteger(value) && value >= 0 && value <= 127;

/** Whether a pad of this kind can be lit: a program change has no "off". */
export const canLight = (trigger: MidiTrigger): boolean => trigger.kind !== "program";

/**
 * The preferences with one of a binding's feedback values changed. The other value stays what the
 * binding showed (its own, or the controller's `fallback`), so the binding then holds both.
 */
export const setBindingFeedback = (
  prefs: ControlPrefs,
  index: number,
  fallback: Feedback,
  change: Partial<Feedback>,
): ControlPrefs => ({
  ...prefs,
  midi: {
    ...prefs.midi,
    bindings: prefs.midi.bindings.map((binding, i) =>
      i === index ? { ...binding, feedback: { ...fallback, ...binding.feedback, ...change } } : binding,
    ),
  },
});

export const removeBinding = (prefs: ControlPrefs, index: number): ControlPrefs => ({
  ...prefs,
  midi: { ...prefs.midi, bindings: prefs.midi.bindings.filter((_, i) => i !== index) },
});

/** The OSC addresses Flux answers to, for the user to copy into a Stream Deck button. */
export const OSC_ADDRESSES: { address: string; numbered?: boolean }[] = [
  { address: "/flux/go" },
  { address: "/flux/devamp" },
  { address: "/flux/devamp/cancel" },
  { address: "/flux/play" },
  { address: "/flux/pause" },
  { address: "/flux/toggle" },
  { address: "/flux/stop" },
  { address: "/flux/cue/next" },
  { address: "/flux/cue/prev" },
  { address: "/flux/cue/goto", numbered: true },
  { address: "/flux/seek/frames", numbered: true },
  { address: "/flux/seek/seconds", numbered: true },
  { address: "/flux/timeline/next" },
  { address: "/flux/timeline/prev" },
  { address: "/flux/timeline/select", numbered: true },
  { address: "/flux/view/edit" },
  { address: "/flux/view/live" },
];
