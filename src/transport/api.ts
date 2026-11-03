import { Channel, invoke } from "@tauri-apps/api/core";
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
  /** The device renders audio faster than real time (ALSA's null device does): the clock is wrong. */
  clockUnpaced: boolean;
}

/** The loudest sample of one track since the previous event, as a linear amplitude. */
export interface TrackPeak {
  id: string;
  peak: number;
}

/** Where a running output sends what it reports, about 30 times a second each, over lanes of their own. */
export interface OutputLanes {
  /** The position, when it moved. */
  onPosition: (position: Position) => void;
  /** The peak of each track; silence once after the sound stops, then nothing. */
  onLevels: (tracks: TrackPeak[]) => void;
}

/**
 * Opens the devices the project's `output` names; replaces a running output. Position and levels
 * come over one channel each, which live as long as this output does, so the meters can never hold
 * up the timecode.
 */
export function startOutput(project: Project, lanes: OutputLanes): Promise<OutputInfo> {
  const position = new Channel<Position>();
  position.onmessage = lanes.onPosition;
  const levels = new Channel<TrackPeak[]>();
  levels.onmessage = lanes.onLevels;
  return invoke<OutputInfo>("output_start", { project, onPosition: position, onLevels: levels });
}

/**
 * Sends the project's current clips to a running output, after tracks were muted or clips moved.
 * Resolves to the files that could not be used.
 */
export function setClips(project: Project): Promise<string[]> {
  return invoke<string[]>("output_set_clips", { project });
}

/**
 * Moves one track's fader on a running output, without rebuilding the programme, so it can follow
 * a drag. Committing the edit to the project is what makes it stick.
 */
export function setTrackGain(trackId: string, gainDb: number): Promise<void> {
  return invoke("output_set_track_gain", { trackId, gainDb });
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
