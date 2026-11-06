// The shape of a clip's fades. The formulas mirror `fade_shape` in the engine's mixer, which is
// what is heard; the timeline draws the same curve from them.

import type { FadeCurve } from "../project/types";

export const FADE_CURVES: readonly FadeCurve[] = ["linear", "equalPower", "sCurve", "exponential"];

/**
 * The gain of a fade `x` of the way up from silence (0 is silent, 1 is full). A fade in and a fade
 * out share it: a fade out is read from its end.
 */
export function fadeShape(curve: FadeCurve, x: number): number {
  switch (curve) {
    case "equalPower":
      return Math.sin(x * (Math.PI / 2));
    case "sCurve":
      return 0.5 - 0.5 * Math.cos(x * Math.PI);
    case "exponential":
      return x * x;
    default:
      return x;
  }
}

/**
 * The points of an SVG polyline that draws a fade `width` pixels wide in a box `height` tall, silence
 * at the bottom. A fade in starts at the left edge of the box and rises to its right edge; a fade out
 * starts at the left edge at full level and falls to the right.
 */
export function fadePoints(curve: FadeCurve, direction: "in" | "out", width: number, height: number, steps = 24): string {
  const points: string[] = [];
  for (let step = 0; step <= steps; step++) {
    const along = step / steps;
    const gain = fadeShape(curve, direction === "in" ? along : 1 - along);
    points.push(`${round(along * width)},${round((1 - gain) * height)}`);
  }
  return points.join(" ");
}

const round = (value: number) => Math.round(value * 100) / 100;
