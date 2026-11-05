/**
 * Where a dragged track lands: its index in the track order once dropped.
 * `heights` are the rows' heights in the current order, `from` the dragged row and `dy` how far the
 * pointer has moved vertically. The row goes after every other row whose middle its own middle has passed.
 */
export function dropIndex(heights: readonly number[], from: number, dy: number): number {
  let top = 0;
  const middles = heights.map((height) => {
    const middle = top + height / 2;
    top += height;
    return middle;
  });
  const dragged = middles[from] + dy;
  return middles.filter((middle, index) => index !== from && middle < dragged).length;
}

/** The y, in the rows as they are now, of the line that marks where the dragged row will land. */
export function dropLineY(heights: readonly number[], from: number, to: number): number {
  const above = to <= from ? to : to + 1;
  return heights.slice(0, above).reduce((sum, height) => sum + height, 0);
}
