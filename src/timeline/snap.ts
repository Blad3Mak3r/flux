// The magnet: moves a position onto the closest vertical line of the grid when it is near one.

/** How close, in pixels, a position must be to a line for the magnet to take it. */
export const SNAP_PX = 8;

/**
 * `frame` moved onto the closest multiple of `spacing` (frames between lines) when that is within
 * `SNAP_PX` on screen, else left as it is. The result is a whole frame, as every position is.
 */
export function snapToSpacing(frame: number, spacing: number, pxPerFrame: number): number {
  if (!(spacing > 0)) return frame;
  const nearest = Math.round(frame / spacing) * spacing;
  return Math.abs(nearest - frame) * pxPerFrame <= SNAP_PX ? Math.round(nearest) : frame;
}
