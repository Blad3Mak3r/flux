// A clip's volume envelope: breakpoints over the clip, edited in the timeline. The evaluation
// mirrors `Envelope::gain_at` in the engine's mixer, which is what is heard; the line drawn over the
// clip comes from it.

import type { EnvCurve, EnvPoint } from "../project/types";
import { ENVELOPE_MAX_DB, ENVELOPE_MIN_DB, MAX_ENVELOPE_POINTS } from "../project/types.ts";

export const ENVELOPE_CURVES: readonly EnvCurve[] = ["linear", "smooth", "fast", "slow", "hold"];

const gainFromDb = (db: number) => 10 ** (db / 20);

/** The gain a point sets, linear; silence is exactly 0. */
const pointGain = (point: EnvPoint) => (point.gainDb <= ENVELOPE_MIN_DB ? 0 : gainFromDb(point.gainDb));

/** How far along the line the curve of a segment is `t` of the way (0 to 1) through it. */
export function curveShape(curve: EnvCurve | undefined, t: number): number {
  switch (curve) {
    case "smooth":
      return t * t * (3 - 2 * t);
    case "fast":
      return 1 - (1 - t) * (1 - t);
    case "slow":
      return t * t;
    case "hold":
      // The engine ramps to the next point over a few ms; at this scale that is the step at the end.
      return t < 1 ? 0 : 1;
    default:
      return t;
  }
}

/** The linear gain of `points` at `tick`: the engine's `gain_at`, in dB between two points unless one is silence. */
export function envelopeGainAt(points: readonly EnvPoint[], tick: number): number {
  if (points.length === 0) return 1;
  let next = points.findIndex((point) => point.at > tick);
  if (next === -1) next = points.length;
  if (next === 0) return pointGain(points[0]);
  const from = points[next - 1];
  const to = points[next];
  if (!to) return pointGain(from);
  const u = curveShape(from.curve, (tick - from.at) / (to.at - from.at));
  const [a, b] = [pointGain(from), pointGain(to)];
  return a > 0 && b > 0 ? gainFromDb(from.gainDb + (to.gainDb - from.gainDb) * u) : a + (b - a) * u;
}

/** `envelopeGainAt` in dB, never below silence. */
export function envelopeDbAt(points: readonly EnvPoint[], tick: number): number {
  const gain = envelopeGainAt(points, tick);
  return gain <= 0 ? ENVELOPE_MIN_DB : Math.max(ENVELOPE_MIN_DB, 20 * Math.log10(gain));
}

/** The vertices of the line drawn over a clip: every point, with `steps` more inside each curved segment. */
export function envelopeLine(points: readonly EnvPoint[], steps = 12): { at: number; db: number }[] {
  const line: { at: number; db: number }[] = [];
  points.forEach((point, index) => {
    line.push({ at: point.at, db: point.gainDb });
    const next = points[index + 1];
    if (!next) return;
    // A straight line in dB between two points needs no more; the others bend.
    const bent = point.curve !== undefined && point.curve !== "linear";
    const silent = point.gainDb <= ENVELOPE_MIN_DB || next.gainDb <= ENVELOPE_MIN_DB;
    if (point.curve === "hold") line.push({ at: next.at, db: point.gainDb });
    else if (bent || silent) {
      for (let step = 1; step < steps; step++) {
        const at = point.at + ((next.at - point.at) * step) / steps;
        line.push({ at, db: envelopeDbAt(points, at) });
      }
    }
  });
  return line;
}

export const clampEnvelopeDb = (db: number) =>
  Math.min(Math.max(Math.round(db * 10) / 10, ENVELOPE_MIN_DB), ENVELOPE_MAX_DB);

/**
 * `points` with one more at `at` ticks, in its place in the order. The same list comes back when
 * there is a point there already or the envelope is full.
 */
export function insertPoint(points: readonly EnvPoint[], at: number, gainDb: number): EnvPoint[] {
  const place = Math.max(0, Math.round(at));
  if (points.length >= MAX_ENVELOPE_POINTS || points.some((point) => point.at === place)) return [...points];
  const point: EnvPoint = { at: place, gainDb: clampEnvelopeDb(gainDb) };
  const index = points.findIndex((other) => other.at > place);
  return index === -1 ? [...points, point] : [...points.slice(0, index), point, ...points.slice(index)];
}

/** `points` with the point `index` moved to `at` ticks and `gainDb`, kept between its neighbours. */
export function movePoint(points: readonly EnvPoint[], index: number, at: number, gainDb: number): EnvPoint[] {
  const current = points[index];
  if (!current) return [...points];
  const before = points[index - 1]?.at ?? -1;
  const after = points[index + 1]?.at ?? Number.POSITIVE_INFINITY;
  const place = Math.min(Math.max(Math.round(at), before + 1), after - 1);
  const moved: EnvPoint = { ...current, at: Math.max(0, place), gainDb: clampEnvelopeDb(gainDb) };
  return points.map((point, i) => (i === index ? moved : point));
}

export function removePoint(points: readonly EnvPoint[], index: number): EnvPoint[] {
  return points.filter((_, i) => i !== index);
}

/** The point `index` with the curve to the next point; a straight line is stored as no value. */
export function setPointCurve(points: readonly EnvPoint[], index: number, curve: EnvCurve): EnvPoint[] {
  return points.map((point, i) => {
    if (i !== index) return point;
    const { curve: _previous, ...rest } = point;
    return curve === "linear" ? rest : { ...rest, curve };
  });
}
