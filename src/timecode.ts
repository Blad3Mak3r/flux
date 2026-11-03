// Timecode maths for the UI. Mirrors src-tauri/src/engine/timecode.rs: keep the two in step.
// Erasable syntax only, so `node --test` can run it without a build step.

import type { FrameRate } from "./project/types";

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

/** Converts timecode to a frame count, or `null` when a field does not exist at `rate`. */
export function timecodeToFrames(tc: Timecode, rate: FrameRate): number | null {
  const fps = nominalFps(rate);
  if (tc.hours < 0 || tc.hours > HOURS_PER_DAY - 1) return null;
  if (tc.minutes < 0 || tc.minutes > 59 || tc.seconds < 0 || tc.seconds > 59) return null;
  if (tc.frames < 0 || tc.frames > fps - 1) return null;

  const totalMinutes = tc.hours * 60 + tc.minutes;
  const nominal = totalMinutes * 60 * fps + tc.seconds * fps + tc.frames;
  if (!isDropFrame(rate)) return nominal;
  if (tc.minutes % 10 !== 0 && tc.seconds === 0 && tc.frames < 2) return null;
  return nominal - 2 * (totalMinutes - Math.floor(totalMinutes / 10));
}

const two = (n: number) => String(n).padStart(2, "0");

/** `HH:MM:SS:FF`, with `;` before the frames for drop-frame. */
export function formatTimecode(tc: Timecode, rate: FrameRate): string {
  const separator = isDropFrame(rate) ? ";" : ":";
  return `${two(tc.hours)}:${two(tc.minutes)}:${two(tc.seconds)}${separator}${two(tc.frames)}`;
}

export function formatFrames(frames: number, rate: FrameRate): string {
  return formatTimecode(framesToTimecode(frames, rate), rate);
}

/**
 * Reads `HH:MM:SS:FF` (`:`, `;` or `.` between the fields, or only digits) into a frame count.
 * Fewer fields fill from the right, so `1:30` is one second and thirty frames... at 30 fps; digits
 * alone fill from the right in pairs, as on a hardware keypad.
 */
export function parseTimecode(text: string, rate: FrameRate): number | null {
  const trimmed = text.trim();
  if (!/^[\d:;.]+$/.test(trimmed)) return null;
  let parts: number[];
  if (/^\d+$/.test(trimmed)) {
    const padded = trimmed.padStart(8, "0").slice(-8);
    parts = padded.match(/\d{2}/g)!.map(Number);
  } else {
    const fields = trimmed.split(/[:;.]/).map((field) => (field === "" ? NaN : Number(field)));
    if (fields.length > 4 || fields.some(Number.isNaN)) return null;
    parts = [...Array<number>(4 - fields.length).fill(0), ...fields];
  }
  const [hours, minutes, seconds, frames] = parts;
  return timecodeToFrames({ hours, minutes, seconds, frames }, rate);
}
