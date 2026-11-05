// How the timeline scrolls to keep the playhead in view while the show plays.
//
// The playhead moves freely until it reaches an anchor a third of the way across the view, then
// stays there and the timeline flows under it, continuously, one pixel at a time. When the
// playhead leaves the view for another reason (a seek, Stop, the view being resized) the scroll
// does not jump: it slides in. Plain functions, no React, so it can be tested.

/** Where the playhead rests while the timeline flows, as a fraction of the view's width. */
export const ANCHOR = 0.35;
/** The playhead is brought back before it gets this close to the left edge. */
const EDGE = 24;
/** How long a slide takes to settle (time constant, ms). */
const GLIDE_MS = 140;
/** A slide starts at most this many view widths away, so a seek across the whole show is quick. */
const MAX_SLIDE = 1.5;

export interface Follower {
  /** Takes the scroll the view really has, after the user moved it or the view was zoomed. */
  sync(scroll: number): void;
  /** The scroll to give the view for a playhead at `x`. Call once per drawn frame. */
  step(x: number, now: number, view: { width: number; max: number }): number;
  /** A slide is still settling, so another frame is due even if nothing moves. */
  readonly sliding: boolean;
}

export function createFollower(initial = 0): Follower {
  let base = initial;
  let offset = 0;
  let at: number | null = null;

  return {
    sync(scroll) {
      base = scroll;
      offset = 0;
    },
    step(x, now, { width, max }) {
      const anchor = width * ANCHOR;
      const dt = at === null ? 0 : Math.max(now - at, 0);
      at = now;
      const clamp = (value: number) => Math.min(Math.max(value, 0), Math.max(max, 0));

      const from = x - base;
      const wanted = from >= EDGE && from <= anchor ? base : clamp(x - anchor);
      // Continuous flow moves the base a little each frame; anything bigger is a jump to slide over.
      if (Math.abs(wanted - base) > Math.max(160, width * 0.2)) {
        const limit = width * MAX_SLIDE;
        offset = Math.min(Math.max(offset + base - wanted, -limit), limit);
      }
      base = wanted;
      offset = Math.abs(offset) < 0.25 ? 0 : offset * Math.exp(-dt / GLIDE_MS);
      return clamp(base + offset);
    },
    get sliding() {
      return offset !== 0;
    },
  };
}
