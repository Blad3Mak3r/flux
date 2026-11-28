// The one place the Tauri API is faked for component tests. `setup.ts` installs these in place of
// `@tauri-apps/api`; a test answers the commands it cares about with `answer` and reads what the UI
// asked with `calls`. Commands nobody answered get a harmless default (see `DEFAULTS`).

export type Handler = unknown | ((args: Record<string, unknown>) => unknown);

interface Call {
  command: string;
  args: Record<string, unknown>;
}

export const OUTPUT_INFO = {
  device: "Test device",
  sampleRate: 48000,
  channels: 2,
  ltc: [],
  mtc: [],
  metronome: null,
  operator: null,
  midiClock: null,
  cues: [],
  warnings: [],
};

/** What the backend says to commands a test did not mention: empty lists and "nothing there". */
const DEFAULTS: Record<string, Handler> = {
  devices_list_hosts: [],
  devices_list_audio: [],
  devices_list_midi: [],
  devices_find_audio: null,
  devices_find_midi: null,
  output_attach: null,
  output_start: OUTPUT_INFO,
  live_cues: [],
  show_lock_get: false,
};

let handlers = new Map<string, Handler>();
let log: Call[] = [];
let listeners = new Map<string, Set<(event: { payload: unknown }) => void>>();

/** Forget every answer, call and listener; run before each test. */
export function resetTauri(): void {
  handlers = new Map(Object.entries(DEFAULTS));
  log = [];
  listeners = new Map();
}
resetTauri();

/** Answer `command` with a value, or with a function of its arguments (it may return a promise or throw). */
export function answer(command: string, handler: Handler): void {
  handlers.set(command, handler);
}

export async function invoke(command: string, args: Record<string, unknown> = {}): Promise<unknown> {
  log.push({ command, args });
  const handler = handlers.get(command);
  return typeof handler === "function" ? handler(args) : (handler ?? null);
}

/** The arguments of every call of `command`, oldest first. */
export const calls = (command: string): Record<string, unknown>[] => log.filter((call) => call.command === command).map((call) => call.args);

/** Every command asked so far, in order. */
export const commands = (): string[] => log.map((call) => call.command);

/** A channel of the backend: the UI sets `onmessage`, the test (through `send`) plays the backend. */
export class Channel<T = unknown> {
  onmessage: (message: T) => void = () => undefined;
}

/**
 * Plays the backend sending `message` over the channel the UI passed to `command` under `name`
 * (e.g. `send("output_start", "onPosition", position)`): the newest call that carried one.
 */
export function send(command: string, name: string, message: unknown): void {
  const channel = calls(command).map((args) => args[name]).filter(Boolean).pop() as Channel | undefined;
  if (!channel) throw new Error(`no ${command} call carried the channel ${name}`);
  channel.onmessage(message);
}

export async function listen(event: string, callback: (event: { payload: unknown }) => void): Promise<() => void> {
  const set = listeners.get(event) ?? new Set();
  set.add(callback);
  listeners.set(event, set);
  return () => void set.delete(callback);
}

/** Plays the backend emitting `event`. */
export function emit(event: string, payload?: unknown): void {
  listeners.get(event)?.forEach((callback) => callback({ payload }));
}

/** A window that does nothing; enough for the controls that ask it things. */
export const fakeWindow = {
  isMaximized: async () => false,
  onResized: async () => () => undefined,
  onCloseRequested: async () => () => undefined,
  onDragDropEvent: async () => () => undefined,
  minimize: async () => undefined,
  toggleMaximize: async () => undefined,
  destroy: async () => undefined,
  close: async () => undefined,
};
