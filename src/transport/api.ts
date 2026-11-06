import { Channel, invoke } from "@tauri-apps/api/core";
import type { LiveState } from "../live/api";
import type { Project, Tempo } from "../project/types";

// Mirrors src-tauri/crates/flux-engine/src/output.rs and src-tauri/src/transport_commands.rs.

export type TransportState = "stopped" | "playing" | "paused";

export interface OutputInfo {
  /** Name of the audio device that was opened. */
  device: string;
  sampleRate: number;
  channels: number;
  /** One entry per `output.ltc` of the project, in the same order. */
  ltc: LtcInfo[];
  /** One entry per `output.mtc` of the project, in the same order. */
  mtc: MtcInfo[];
  /** Where the metronome plays; `null` when the project has no metronome output. */
  metronome: MetronomeInfo | null;
  /** One entry per MIDI port and OSC destination the cue tracks of the timeline send to. */
  cues: CueInfo[];
  /** Things that did not stop the output, e.g. a missing MIDI port. */
  warnings: string[];
}

/** How one destination of the cue tracks ended up; when it did not open, `target` is the name the project holds. */
export interface CueInfo {
  kind: "midi" | "osc";
  target: string;
  ok: boolean;
}

/** How one LTC output ended up; when it did not open, `device` is the name the project holds. */
export interface LtcInfo {
  device: string;
  /** 1-based interface channel. */
  channel: number;
  ok: boolean;
}

/** How the metronome ended up; when it did not open, `device` is the name the project holds. */
export interface MetronomeInfo {
  device: string;
  ok: boolean;
}

/** How one MTC output ended up; when it did not open, `port` is the name the project holds. */
export interface MtcInfo {
  port: string;
  ok: boolean;
}

export interface Position {
  state: TransportState;
  /** Frame count of the timecode being played, including the project's start offset. */
  frame: number;
  positionSamples: number;
  /** The audio device failed or went away while running. */
  deviceLost: boolean;
  /** The LTC device, when it is not the programme's, failed or went away. */
  ltcLost: boolean;
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
  /** Where the show stands among its cues, when that changed. */
  onLive: (state: LiveState) => void;
}

/**
 * Opens the devices the project's `output` names; replaces a running output. Position, levels and
 * the state of the show come over one channel each, which live as long as this output does, so the
 * meters can never hold up the timecode. With `advance`, the backend makes the next timeline the
 * active one when the show runs to its end (see `bindTimelineAdvance`).
 */
export function startOutput(project: Project, advance: boolean, lanes: OutputLanes): Promise<OutputInfo> {
  const position = new Channel<Position>();
  position.onmessage = lanes.onPosition;
  const levels = new Channel<TrackPeak[]>();
  levels.onmessage = lanes.onLevels;
  const live = new Channel<LiveState>();
  live.onmessage = lanes.onLive;
  return invoke<OutputInfo>("output_start", {
    project,
    advance,
    onPosition: position,
    onLevels: levels,
    onLive: live,
  });
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

/** Moves the metronome of a running output to the active timeline's tempo and click switch. */
export function setTempo(tempo: Tempo): Promise<void> {
  return invoke("output_set_tempo", { tempo });
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
