// The position to draw, between the backend's messages. The backend reports about 30 times a
// second and a screen draws 60 or more, so drawing only what arrives would move the playhead and
// the scrolling timeline in visible steps. This projects the last report forward with the wall
// clock, which is what the backend itself does between audio callbacks, and blends each new report
// in over a short time, so a late or early message never makes the picture stop or step back.
//
// It only decides what to draw: the readout, the timecode and every command keep using the
// backend's own position. Plain functions, no React, so it can be tested.

import type { Position } from "./api";

/** The engine's sample rate (`ENGINE_RATE` in src-tauri/crates/flux-core/src/lib.rs). */
export const ENGINE_RATE = 48_000;

/** How long a difference between the projection and a new report takes to fade away. */
const BLEND_MS = 120;
/** A report further than this (in frames) from the projection is a jump, not jitter: it is taken as is. */
const MAX_BLEND_FRAMES = 2;
/** Never project further past the last report than this: a stalled backend must not run the picture away. */
const MAX_AHEAD_MS = 250;

export interface DisplayClock {
  /** A report from the backend. `fps` is the real frame rate (29.97 for drop-frame) and `start` the timeline origin. */
  feed(position: Position, now: number, fps: number, start: number): void;
  /** Forgets the projection, after a seek the UI made itself: the position it set is the one to draw. */
  jump(): void;
  /** The frame to draw at `now` (fractional, absolute), or `null` when only the stored position is known. */
  read(now: number): number | null;
}

export function createDisplayClock(): DisplayClock {
  let last: { frame: number; at: number; fps: number } | null = null;
  let blend = { frames: 0, at: 0 };

  const read = (now: number): number | null => {
    if (!last) return null;
    const ahead = Math.min(Math.max(now - last.at, 0), MAX_AHEAD_MS);
    const fade = Math.exp(-Math.max(now - blend.at, 0) / BLEND_MS);
    return last.frame + (ahead * last.fps) / 1000 + blend.frames * fade;
  };

  return {
    feed(position, now, fps, start) {
      if (position.state !== "playing") {
        last = null;
        blend = { frames: 0, at: now };
        return;
      }
      const frame = start + (position.positionSamples * fps) / ENGINE_RATE;
      const drawn = read(now);
      const difference = drawn === null ? 0 : drawn - frame;
      blend = { frames: Math.abs(difference) <= MAX_BLEND_FRAMES ? difference : 0, at: now };
      last = { frame, at: now, fps };
    },
    jump() {
      last = null;
      blend = { frames: 0, at: 0 };
    },
    read,
  };
}

/** The clock the timeline draws from; the output's lanes feed it. */
export const displayClock = createDisplayClock();
