import { useEffect } from "react";
import type { RefObject } from "react";
import { useT } from "../i18n";
import { useStore } from "../store";
import { HEADER_W } from "../timeline/layout";
import { positionStore } from "../transport/playback";

/** The line at the current position. While playing, it keeps itself in view by turning pages. */
export function Playhead({
  parked,
  start,
  pxPerFrame,
  scroller,
}: {
  parked: number;
  start: number;
  pxPerFrame: number;
  scroller: RefObject<HTMLDivElement | null>;
}) {
  const t = useT();
  const position = useStore(positionStore);
  const frame = position?.frame ?? parked;
  const x = Math.round((frame - start) * pxPerFrame);
  const playing = position?.state === "playing";

  useEffect(() => {
    const box = scroller.current;
    if (!box || !playing) return;
    const visibleLeft = box.scrollLeft;
    const visibleRight = visibleLeft + box.clientWidth - HEADER_W;
    if (x > visibleRight - 40 || x < visibleLeft) box.scrollLeft = Math.max(0, x - 120);
  }, [x, playing, scroller]);

  return (
    <div
      className="playhead"
      data-playing={playing}
      style={{ transform: `translateX(${HEADER_W + x}px)` }}
      role="presentation"
      title={t("timeline.playhead")}
    />
  );
}
