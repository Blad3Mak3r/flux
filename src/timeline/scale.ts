import type { FrameRate } from "../project/types";
import { nominalFps } from "../timecode.ts";

export const MIN_PX_PER_FRAME = 0.04;
export const MAX_PX_PER_FRAME = 40;
export const DEFAULT_PX_PER_FRAME = 1.6;

/** Smallest gap between labelled ticks, in pixels. */
const MIN_MAJOR_PX = 96;
const MIN_MINOR_PX = 12;

export interface Ticks {
  /** Frames between labelled ticks. */
  major: number;
  /** Frames between unlabelled ticks, or `null` when they would be too close. */
  minor: number | null;
  /** The major interval is a whole number of seconds, so labels can drop the frames. */
  wholeSeconds: boolean;
}

function candidates(rate: FrameRate): number[] {
  const fps = nominalFps(rate);
  const seconds = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600, 1_800, 3_600];
  return [1, 2, 5, 10, ...seconds.map((s) => s * fps)].filter(
    (frames, index, all) => index === 0 || frames > all[index - 1],
  );
}

/** Picks tick spacing for the zoom level so labels never crowd each other. */
export function pickTicks(pxPerFrame: number, rate: FrameRate): Ticks {
  const all = candidates(rate);
  const fps = nominalFps(rate);
  const major = all.find((frames) => frames * pxPerFrame >= MIN_MAJOR_PX) ?? all[all.length - 1];
  const divisor = [5, 4, 2].find((d) => major % d === 0 && (major / d) * pxPerFrame >= MIN_MINOR_PX);
  return {
    major,
    minor: divisor ? major / divisor : null,
    wholeSeconds: major >= fps && major % fps === 0,
  };
}

export function clampZoom(pxPerFrame: number): number {
  return Math.min(MAX_PX_PER_FRAME, Math.max(MIN_PX_PER_FRAME, pxPerFrame));
}

/** Pixel column of a frame. The ruler, graticule, clips, flags and playhead all use this rounding. */
export function frameX(frame: number, pxPerFrame: number): number {
  return Math.round(frame * pxPerFrame);
}

export interface TickPositions {
  /** Frames of the labelled ticks. */
  major: number[];
  /** Frames of the unlabelled ticks, without the ones that coincide with a major tick. */
  minor: number[];
}

/** Every tick up to `width` pixels, so the ruler and the graticule are drawn from the same list. */
export function tickPositions(pxPerFrame: number, rate: FrameRate, width: number): TickPositions {
  const ticks = pickTicks(pxPerFrame, rate);
  const frames = Math.ceil(width / pxPerFrame);
  const major: number[] = [];
  const minor: number[] = [];
  for (let f = 0; f <= frames; f += ticks.major) major.push(f);
  if (ticks.minor !== null) {
    for (let f = 0; f <= frames; f += ticks.minor) if (f % ticks.major !== 0) minor.push(f);
  }
  return { major, minor };
}
