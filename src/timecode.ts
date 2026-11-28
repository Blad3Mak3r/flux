// Timecode display maths for the UI: frames to text, redrawn every frame, so it stays here instead of crossing the IPC.
// Mirrors src-tauri/crates/flux-core/src/timecode.rs: keep the two in step. Typed text is parsed by the backend (`parse_timecode`).
// Erasable syntax only, so `node --test` can run it without a build step.

import type { FrameRate } from "./project/types";
import { pad2 } from "./format.ts";

export interface Timecode {
  hours: number;
  minutes: number;
  seconds: number;
  frames: number;
}

/** Frames in 10 minutes of 29.97 drop-frame: 2 frames are dropped every minute except each tenth. */
const DF_FRAMES_PER_10_MIN = 17_982;
const DF_FRAMES_PER_DROP_MINUTE = 1_798;
const HOURS_PER_DAY = 24;

/** Frames counted per second on the display (30 for 29.97 drop-frame). */
export function nominalFps(rate: FrameRate): number {
  switch (rate) {
    case "fps24":
      return 24;
    case "fps25":
      return 25;
    case "fps2997Drop":
    case "fps30":
      return 30;
  }
}

export function isDropFrame(rate: FrameRate): boolean {
  return rate === "fps2997Drop";
}

/** The real rate in frames per second. */
export function realFps(rate: FrameRate): number {
  return rate === "fps2997Drop" ? 30_000 / 1_001 : nominalFps(rate);
}

export function framesPerHour(rate: FrameRate): number {
  return isDropFrame(rate) ? 6 * DF_FRAMES_PER_10_MIN : nominalFps(rate) * 3_600;
}

export function framesPerDay(rate: FrameRate): number {
  return framesPerHour(rate) * HOURS_PER_DAY;
}

/** Converts a frame count to timecode, wrapping after 24 hours. */
export function framesToTimecode(frames: number, rate: FrameRate): Timecode {
  let n = Math.max(0, Math.floor(frames)) % framesPerDay(rate);
  if (isDropFrame(rate)) {
    // Add back the dropped numbers so the count can be split as if it were 30 fps.
    const tens = Math.floor(n / DF_FRAMES_PER_10_MIN);
    const rest = n % DF_FRAMES_PER_10_MIN;
    n += 18 * tens;
    if (rest >= 2) n += 2 * Math.floor((rest - 2) / DF_FRAMES_PER_DROP_MINUTE);
  }
  const fps = nominalFps(rate);
  return {
    hours: Math.floor(n / (fps * 3_600)) % HOURS_PER_DAY,
    minutes: Math.floor(n / (fps * 60)) % 60,
    seconds: Math.floor(n / fps) % 60,
    frames: n % fps,
  };
}

/** `HH:MM:SS:FF`, with `;` before the frames for drop-frame. */
export function formatTimecode(tc: Timecode, rate: FrameRate): string {
  const separator = isDropFrame(rate) ? ";" : ":";
  return `${pad2(tc.hours)}:${pad2(tc.minutes)}:${pad2(tc.seconds)}${separator}${pad2(tc.frames)}`;
}

export function formatFrames(frames: number, rate: FrameRate): string {
  return formatTimecode(framesToTimecode(frames, rate), rate);
}
