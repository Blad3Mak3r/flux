/** Fader and level meter maths. Pure, so it can be tested without a browser. */

/** The fader's bottom: this and anything below is silence ("-inf"). Mirrors the backend. */
export const FADER_MIN_DB = -60;
export const FADER_MAX_DB = 6;

/** Fader position (0 to 1) against dB, as straight pieces: more travel around 0 dB than near -inf. */
const TAPER: ReadonlyArray<readonly [position: number, db: number]> = [
  [0, FADER_MIN_DB],
  [0.25, -30],
  [0.5, -12],
  [0.8, 0],
  [1, FADER_MAX_DB],
];

function interpolate(value: number, from: 0 | 1, to: 0 | 1): number {
  const clamped = Math.min(Math.max(value, TAPER[0][from]), TAPER[TAPER.length - 1][from]);
  for (let i = 1; i < TAPER.length; i++) {
    const [a, b] = [TAPER[i - 1], TAPER[i]];
    if (clamped <= b[from]) return a[to] + ((clamped - a[from]) / (b[from] - a[from])) * (b[to] - a[to]);
  }
  return TAPER[TAPER.length - 1][to];
}

/** The gain a fader at `position` (0 to 1) sets, in dB, to a tenth. */
export function faderToDb(position: number): number {
  return roundDb(interpolate(position, 0, 1));
}

export function dbToFader(db: number): number {
  return interpolate(db, 1, 0);
}

function roundDb(db: number): number {
  return Math.round(db * 10) / 10;
}

export function clampFaderDb(db: number): number {
  return Math.min(Math.max(roundDb(db), FADER_MIN_DB), FADER_MAX_DB);
}

/** "+3.0", "-6.0", "0.0", and "-inf" at the bottom of the fader or below any level. */
export function formatDb(db: number): string {
  if (!(db > FADER_MIN_DB)) return "-inf";
  const text = Math.abs(db).toFixed(1);
  return db > 0 ? `+${text}` : db < 0 ? `-${text}` : text;
}

/** dBFS of a linear peak; silence is `-Infinity`. */
export function dbFromLinear(peak: number): number {
  return peak > 0 ? 20 * Math.log10(peak) : -Infinity;
}

export const METER_FLOOR_DB = -60;
export const METER_SEGMENTS = 24;
/** What each lit segment adds, so the top segment ends at 0 dBFS. */
export const SEGMENT_DB = -METER_FLOOR_DB / METER_SEGMENTS;
/** Up to this is green, up to `RED_FROM_DB` yellow, above it red. */
export const YELLOW_FROM_DB = -12;
export const RED_FROM_DB = -3;

export type Tone = "green" | "yellow" | "red";

export function toneOf(db: number): Tone {
  return db <= YELLOW_FROM_DB ? "green" : db <= RED_FROM_DB ? "yellow" : "red";
}

/** dBFS at the top edge of segment `index` (0 is the bottom one). */
export function segmentTopDb(index: number): number {
  return METER_FLOOR_DB + (index + 1) * SEGMENT_DB;
}

/** How many segments are lit for a level: one as soon as it clears the floor. */
export function litSegments(db: number): number {
  if (!(db > METER_FLOOR_DB)) return 0;
  return Math.min(METER_SEGMENTS, Math.ceil((db - METER_FLOOR_DB) / SEGMENT_DB));
}

/** Index of the segment a level falls in, or -1 below the floor. */
export function segmentOf(db: number): number {
  return litSegments(db) - 1;
}

/** 0 dBFS is the largest sample a device can play: at or over it the signal clips. */
export const CLIP_DB = 0;

/** What a meter remembers between frames. */
export interface Ballistics {
  /** The bar, in dBFS. */
  level: number;
  /** The peak-hold marker, in dBFS. */
  hold: number;
  /** Milliseconds since the hold marker was last pushed up. */
  holdAge: number;
  /** Stays set until the user resets it. */
  clip: boolean;
}

export const BALLISTICS_START: Ballistics = {
  level: -Infinity,
  hold: -Infinity,
  holdAge: 0,
  clip: false,
};

/** The bar falls this fast; it rises at once, so a short peak is never missed. */
export const FALL_DB_PER_S = 24;
/** The hold marker stays put this long, then falls slowly. */
export const HOLD_MS = 1500;
export const HOLD_FALL_DB_PER_S = 12;

/**
 * Advances a meter by `dtMs` given the peak in dBFS since the last step. With `instant` (reduced
 * motion) the bar follows the signal and the marker does not slide: it jumps down after the hold.
 */
export function stepBallistics(
  state: Ballistics,
  peakDb: number,
  dtMs: number,
  instant = false,
): Ballistics {
  const seconds = dtMs / 1000;
  const level = instant ? peakDb : Math.max(peakDb, state.level - FALL_DB_PER_S * seconds);
  let { hold, holdAge } = state;
  if (peakDb >= hold) {
    hold = peakDb;
    holdAge = 0;
  } else {
    holdAge += dtMs;
    if (holdAge > HOLD_MS) hold = instant ? level : Math.max(level, hold - HOLD_FALL_DB_PER_S * seconds);
  }
  const floored = (db: number) => (db < METER_FLOOR_DB ? -Infinity : db);
  return {
    level: floored(level),
    hold: floored(hold),
    holdAge,
    clip: state.clip || peakDb >= CLIP_DB,
  };
}

/** Whether a meter has settled at silence, so its animation loop can stop. */
export function isSettled(state: Ballistics): boolean {
  return state.level === -Infinity && state.hold === -Infinity;
}
