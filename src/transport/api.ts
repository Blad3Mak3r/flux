import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Project } from "../project/types";

// Mirrors src-tauri/src/engine/output.rs and src-tauri/src/transport_commands.rs.

export type TransportState = "stopped" | "playing" | "paused";

export interface OutputInfo {
  /** Name of the audio device that was opened. */
  device: string;
  sampleRate: number;
  channels: number;
  /** 1-based interface channel carrying LTC. */
  ltcChannel: number | null;
  mtcPort: string | null;
  /** Things that did not stop the output, e.g. a missing MIDI port. */
  warnings: string[];
}

export interface Position {
  state: TransportState;
  /** Frame count of the timecode being played, including the project's start offset. */
  frame: number;
  positionSamples: number;
  /** The audio device failed or went away while running. */
  deviceLost: boolean;
}

/** Opens the devices the project's `output` names; replaces a running output. */
export function startOutput(project: Project): Promise<OutputInfo> {
  return invoke<OutputInfo>("output_start", { project });
}

export function stopOutput(): Promise<void> {
  return invoke("output_stop");
}

export function play(): Promise<void> {
  return invoke("transport_play");
}

export function pause(): Promise<void> {
  return invoke("transport_pause");
}

/** Stops and goes back to the start of the timeline. */
export function stop(): Promise<void> {
  return invoke("transport_stop");
}

/** Moves to a timecode given as a frame count. */
export function seek(frame: number): Promise<void> {
  return invoke("transport_seek", { frame });
}

/** The current position, or `null` when no output is running. */
export function position(): Promise<Position | null> {
  return invoke<Position | null>("transport_position");
}

/** Calls `handler` about 30 times a second while the position changes. */
export function onPosition(handler: (position: Position) => void): Promise<UnlistenFn> {
  return listen<Position>("transport-position", (event) => handler(event.payload));
}
