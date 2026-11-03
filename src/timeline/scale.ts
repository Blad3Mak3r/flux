import type { FrameRate } from "../project/types";
import { nominalFps } from "../timecode";

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
