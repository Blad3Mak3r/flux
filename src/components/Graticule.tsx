import { useMemo } from "react";
import type { FrameRate } from "../project/types";
import { frameX, tickPositions } from "../timeline/scale";

/**
 * The vertical lines behind the lanes. They come from the same tick list and the same pixel
 * rounding as the ruler, so they sit exactly under its ticks at every rate and zoom. The paths
 * are one unit tall and stretched to the lanes' height, with strokes that do not scale.
 */
export function Graticule({ width, pxPerFrame, rate }: { width: number; pxPerFrame: number; rate: FrameRate }) {
  const { major, minor } = useMemo(() => {
    const ticks = tickPositions(pxPerFrame, rate, width);
    const path = (frames: number[]) => frames.map((f) => `M${frameX(f, pxPerFrame) + 0.5} 0V1`).join("");
    return { major: path(ticks.major), minor: path(ticks.minor) };
  }, [width, pxPerFrame, rate]);

  return (
    <svg
      className="graticule"
      width={width}
      height="100%"
      viewBox={`0 0 ${width} 1`}
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <path className="graticule-minor" d={minor} />
      <path className="graticule-major" d={major} />
    </svg>
  );
}
