import { useViewport } from "./layout";
import { visibleRange } from "./scale";
import type { PixelRange } from "./scale";

/**
 * The part of a timeline `total` pixels wide that is on screen, with a screenful each side. It is a
 * new object only when the view moves into another chunk, so what draws from it is not redone at
 * every pixel of scrolling.
 */
export function useVisibleRange(total: number): PixelRange {
  const from = useViewport((view) => visibleRange(view.left, view.width, total).from);
  const to = useViewport((view) => visibleRange(view.left, view.width, total).to);
  return { from, to };
}
