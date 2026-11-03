import type { CSSProperties } from "react";

/**
 * Colours a track can be drawn in. All sit at oklch lightness 0.8, so each reads at 9.6:1 or more
 * on the screen ground (`screen`); none is near vermilion, which only means live or fault.
 * Stored in the project as `#rrggbb`; a track without one uses phosphor (audio) or amber (events).
 */
export const TRACK_COLORS = [
  { id: "teal", hex: "#66d5ba" },
  { id: "cyan", hex: "#5cd0ea" },
  { id: "blue", hex: "#92c1fd" },
  { id: "violet", hex: "#c3b0fd" },
  { id: "magenta", hex: "#eaa3da" },
  { id: "rose", hex: "#f89fbb" },
  { id: "lime", hex: "#c8c26b" },
  { id: "steel", hex: "#b4bfca" },
] as const;

export type TrackColorId = (typeof TRACK_COLORS)[number]["id"];

/** The custom properties the track's header and lane draw with; `undefined` keeps the defaults. */
export function traceStyle(color: string | undefined): CSSProperties | undefined {
  if (!color) return undefined;
  return {
    "--trace": color,
    "--trace-dim": `color-mix(in oklch, ${color} 65%, var(--screen))`,
  } as CSSProperties;
}
