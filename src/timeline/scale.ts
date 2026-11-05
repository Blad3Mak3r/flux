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

/** The beats of a timeline, in the frames it is drawn in. */
export interface BeatGrid {
  bpm: number;
  beatsPerBar: number;
  /** Real frames per second of the timeline, so a beat lasts what the music says. */
  fps: number;
}

/** A labelled tick on the bar ruler. */
export interface BarTick {
  frame: number;
  label: string;
}

export interface BarTickPositions {
  /** Labelled ticks: bars (`3`) or, zoomed in far enough, beats (`3.2`). */
  major: BarTick[];
  /** Unlabelled ticks, without the ones that coincide with a labelled one. */
  minor: number[];
}

/**
 * Beats between labelled ticks (`step`) and between unlabelled ones (`minorStep`, `null` for none):
 * every beat when there is room, else whole bars, doubling until labels cannot crowd.
 */
function barSteps(pxPerFrame: number, grid: BeatGrid): { step: number; minorStep: number | null } {
  const beatPx = beatFrames(grid) * pxPerFrame;
  const perBar = grid.beatsPerBar;
  let step = 1;
  if (beatPx < MIN_MAJOR_PX) {
    step = perBar;
    while (step * beatPx < MIN_MAJOR_PX) step *= 2;
  }
  let minorStep: number | null = null;
  if (step > 1) {
    if (beatPx >= MIN_MINOR_PX) minorStep = 1;
    else if (step > perBar && perBar * beatPx >= MIN_MINOR_PX) minorStep = perBar;
  }
  return { step, minorStep };
}

/**
 * Frames between the closest vertical lines the ruler and graticule draw at this zoom, in the time
 * view (`grid` is `null`) or the bars view. Every drawn line is a multiple of it, so it is what a
 * snap has to land on.
 */
export function lineSpacing(pxPerFrame: number, rate: FrameRate, grid: BeatGrid | null): number {
  if (grid) {
    const { step, minorStep } = barSteps(pxPerFrame, grid);
    return beatFrames(grid) * (minorStep ?? step);
  }
  const ticks = pickTicks(pxPerFrame, rate);
  return ticks.minor ?? ticks.major;
}

/** Frames one beat lasts; not a whole number, which is why positions here are never rounded. */
export const beatFrames = (grid: BeatGrid): number => (60 / grid.bpm) * grid.fps;

/**
 * Every bar line and beat up to `width` pixels, from the origin, spaced like `tickPositions` so
 * labels never crowd. Positions are exact (a beat is rarely a whole frame); `frameX` rounds them
 * to the pixel, so the ruler and the graticule agree.
 */
export function barTickPositions(pxPerFrame: number, grid: BeatGrid, width: number): BarTickPositions {
  const beat = beatFrames(grid);
  const perBar = grid.beatsPerBar;
  const { step, minorStep } = barSteps(pxPerFrame, grid);

  const beats = Math.ceil(width / pxPerFrame / beat);
  const major: BarTick[] = [];
  const minor: number[] = [];
  for (let n = 0; n <= beats; n++) {
    const bar = Math.floor(n / perBar) + 1;
    const inBar = n % perBar;
    if (n % step === 0) major.push({ frame: n * beat, label: inBar === 0 ? String(bar) : `${bar}.${inBar + 1}` });
    else if (minorStep !== null && n % minorStep === 0) minor.push(n * beat);
  }
  return { major, minor };
}
